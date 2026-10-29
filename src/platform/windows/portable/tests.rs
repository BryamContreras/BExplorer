use super::*;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[test]
fn download_target_creation_preserves_a_file_that_arrived_after_reservation() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bexplorer-wpd-exclusive-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("download.bin");
    std::fs::write(&path, b"other writer").unwrap();
    assert_eq!(
        create_download_target(&path).unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"other writer");
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn object_ids_use_the_wpd_string_type_and_keep_unicode() {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::System::Variant::VT_LPWSTR;

    let id = "archivo-árbol-📁";
    let value = portable_object_id_variant(id).expect("convert WPD identifier");
    assert_eq!(
        unsafe { value.as_raw().Anonymous.Anonymous.vt },
        VT_LPWSTR.0
    );
    let text = unsafe { PropVariantToStringAlloc(&value).expect("read identifier") };
    assert_eq!(pwstr_to_string(text), id);
    unsafe { CoTaskMemFree(Some(text.0.cast())) };
}

#[test]
fn nested_com_scopes_leave_the_callers_apartment_initialized() {
    std::thread::spawn(|| {
        use windows::Win32::System::Com::{APTTYPE, APTTYPEQUALIFIER, CoGetApartmentType};
        let outer = PortableComApartment::initialize().expect("initialize COM");
        let inner = PortableComApartment::initialize().expect("enter initialized COM thread");
        drop(inner);
        let mut apartment = APTTYPE::default();
        let mut qualifier = APTTYPEQUALIFIER::default();
        unsafe {
            CoGetApartmentType(&mut apartment, &mut qualifier).expect("outer COM scope stays alive")
        };
        drop(outer);
        // Other tests may keep an implicit MTA alive in the process. Entering
        // an STA still succeeds if both explicit MTA references were balanced.
        use windows::Win32::System::Com::{
            COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize,
        };
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .expect("COM references were balanced");
            CoUninitialize();
        }
    })
    .join()
    .unwrap();
}

#[test]
fn portable_lock_times_out_and_recovers_after_the_operation_finishes() {
    let mutex = std::sync::Mutex::new(());
    let held = mutex.lock().unwrap();
    let result = portable_access_lock_with_timeout(&mutex, std::time::Duration::from_millis(15));
    assert!(result.is_err());
    drop(held);
    assert!(portable_access_lock_with_timeout(&mutex, std::time::Duration::ZERO).is_ok());
}

fn delete_results(
    statuses: &[windows::core::HRESULT],
) -> windows::Win32::Devices::PortableDevices::IPortableDevicePropVariantCollection {
    use windows::Win32::Devices::PortableDevices::{
        IPortableDevicePropVariantCollection, PortableDevicePropVariantCollection,
    };
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::System::Variant::VT_ERROR;
    use windows::core::PROPVARIANT;

    let results: IPortableDevicePropVariantCollection = unsafe {
        CoCreateInstance(
            &PortableDevicePropVariantCollection,
            None,
            CLSCTX_INPROC_SERVER,
        )
        .unwrap()
    };
    for status in statuses {
        let mut raw = *PROPVARIANT::new().as_raw();
        unsafe {
            raw.Anonymous.Anonymous.vt = VT_ERROR.0;
            raw.Anonymous.Anonymous.Anonymous.scode = status.0;
            let value = PROPVARIANT::from_raw(raw);
            results.Add(&value).unwrap();
        }
    }
    results
}

#[test]
fn delete_checks_each_file_instead_of_reporting_partial_failure_as_success() {
    use windows::Win32::Foundation::{E_ACCESSDENIED, S_OK};
    let _apartment = PortableComApartment::initialize().unwrap();
    let success = delete_results(&[S_OK, S_OK]);
    assert_eq!(portable_delete_result_count(Some(&success), 2).unwrap(), 2);
    let partial = delete_results(&[S_OK, E_ACCESSDENIED]);
    let error = portable_delete_result_count(Some(&partial), 2)
        .unwrap_err()
        .to_string();
    assert!(error.contains("1 de 2"));
    assert!(portable_delete_result_count(None, 2).is_err());
    assert!(portable_delete_result_count(Some(&success), 3).is_err());
}

#[test]
#[ignore = "requires BEXPLORER_TEST_PORTABLE_DEVICE and an unlocked USB phone; creates its own temporary folder"]
fn connected_device_delete_then_reload() {
    let (finished, completion) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        exercise_connected_device_delete_then_reload();
        let _ = finished.send(());
    });
    match completion.recv_timeout(std::time::Duration::from_secs(60)) {
        Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            worker.join().expect("device regression test failed");
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!(
                "the MTP driver stopped responding; the isolated fixture is recorded in target/portable-device-test-fixture.json"
            );
        }
    }
}

fn exercise_connected_device_delete_then_reload() {
    let name = std::env::var("BEXPLORER_TEST_PORTABLE_DEVICE")
        .expect("set BEXPLORER_TEST_PORTABLE_DEVICE to the connected phone's name");
    let device = portable_devices()
        .into_iter()
        .find(|device| device.name == name)
        .expect("requested phone must be connected and unlocked");
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/portable-device-test-fixture.json");
    if fixture_path.exists() {
        let previous: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&fixture_path).expect("read prior test fixture"))
                .expect("parse prior test fixture");
        assert_eq!(previous["device_id"].as_str(), Some(device.id.as_str()));
        let folder_name = previous["name"].as_str().expect("prior test folder name");
        assert!(folder_name.starts_with("BExplorer-test-"));
        // MTP object IDs can change after unplugging the phone. Resolve only
        // the exact unique fixture name recorded before the interruption.
        if let Some(storage) = portable_device_objects_result(&device.id, "DEVICE")
            .unwrap()
            .into_iter()
            .find(|object| object.is_folder)
        {
            let folders = portable_device_objects_result(&device.id, &storage.id).unwrap();
            if let Some(info) = folders
                .into_iter()
                .find(|object| object.is_folder && object.name == folder_name)
            {
                eprintln!("Cleaning up the interrupted test's isolated folder");
                portable_delete_objects(&device.id, &[info.id])
                    .expect("clean up prior test folder");
            }
        }
        let _ = std::fs::remove_file(std::env::temp_dir().join(format!("{folder_name}.txt")));
        std::fs::remove_file(&fixture_path).expect("remove prior fixture record");
    }
    let started = Instant::now();
    eprintln!("Opening phone and locating storage");
    let storage = portable_device_objects_result(&device.id, "DEVICE")
        .expect("list phone storage")
        .into_iter()
        .find(|object| object.is_folder)
        .expect("phone must expose storage");
    let connections = connection::CONNECTION_OPENS.load(std::sync::atomic::Ordering::Relaxed);
    let test_name = format!(
        "BExplorer-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    eprintln!("Creating isolated test folder {test_name}");
    let folder_id = {
        let session = PortableDeviceSession::open(&device.id).expect("open phone");
        session
            .create_folder(&storage.id, &test_name)
            .expect("create test folder")
    };
    std::fs::write(
        &fixture_path,
        serde_json::to_vec(&serde_json::json!({
            "device_id": device.id,
            "folder_id": folder_id,
            "name": test_name,
        }))
        .unwrap(),
    )
    .expect("record temporary test folder for cleanup if the driver hangs");

    let local_file = std::env::temp_dir().join(format!("{test_name}.txt"));
    let downloaded_image = std::env::temp_dir().join(format!("{test_name}.png"));
    std::fs::write(&local_file, b"Temporary BExplorer MTP regression test\n")
        .expect("create local fixture");
    let result = (|| -> crate::utils::errors::Result<()> {
        eprintln!("Reading the newly created empty test folder");
        assert!(portable_device_objects_result(&device.id, &folder_id)?.is_empty());
        for round in 0..3 {
            eprintln!("Round {round}: uploading temporary file");
            {
                let session = PortableDeviceSession::open(&device.id)?;
                session.upload_file(&folder_id, &local_file, "delete-test.txt", |_| Ok(()))?;
            }
            let before = portable_device_objects_result(&device.id, &folder_id)?;
            let file = before
                .iter()
                .find(|object| object.name == "delete-test.txt")
                .expect("uploaded fixture must be visible");
            eprintln!("Round {round}: deleting temporary file");
            assert_eq!(
                portable_delete_objects(&device.id, std::slice::from_ref(&file.id))?,
                1
            );
            eprintln!("Round {round}: refreshing folder after deletion");
            let reload_device = device.id.clone();
            let reload_folder = folder_id.clone();
            let after = std::thread::spawn(move || {
                portable_device_objects_result(&reload_device, &reload_folder)
            })
            .join()
            .expect("folder reload worker")?;
            assert!(
                after.iter().all(|object| object.id != file.id),
                "deleted file must disappear"
            );
            eprintln!(
                "Round {round}: refresh completed in {:?}",
                started.elapsed()
            );
        }
        eprintln!("Checking image upload, thumbnail, download and deletion");
        let source_image =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons/appicon.png");
        let image_name = "prueba de miniatura – árbol.png";
        {
            let session = PortableDeviceSession::open(&device.id)?;
            session.upload_file(&folder_id, &source_image, image_name, |_| Ok(()))?;
        }
        let images = portable_device_objects_result(&device.id, &folder_id)?;
        let image = images
            .iter()
            .find(|object| object.name == image_name)
            .expect("uploaded image");
        let thumbnail = portable_device_thumbnail(&device.id, &image.id, 4 * 1024 * 1024, true)
            .expect("read image resource");
        image::load_from_memory(&thumbnail).expect("decode portable image preview");
        {
            let session = PortableDeviceSession::open(&device.id)?;
            session.download_file(&image.id, &downloaded_image, |_| Ok(()))?;
        }
        assert_eq!(
            std::fs::read(&downloaded_image)?,
            std::fs::read(source_image)?
        );
        portable_delete_objects(&device.id, std::slice::from_ref(&image.id))?;
        assert!(portable_device_objects_result(&device.id, &folder_id)?.is_empty());
        eprintln!("Deleting a nonempty nested folder and refreshing its parent");
        let nested = {
            let session = PortableDeviceSession::open(&device.id)?;
            let nested = session.create_folder(&folder_id, "nested-test")?;
            session.upload_file(&nested, &local_file, "nested-file.txt", |_| Ok(()))?;
            nested
        };
        assert_eq!(
            portable_delete_objects(&device.id, std::slice::from_ref(&nested))?,
            1
        );
        let after = portable_device_objects_result(&device.id, &folder_id)?;
        assert!(after.iter().all(|object| object.id != nested));
        assert!(
            connection::CONNECTION_OPENS.load(std::sync::atomic::Ordering::Relaxed) > connections,
            "the regression must cover reopening sessions after deletion"
        );
        Ok(())
    })();
    eprintln!("Removing isolated test folder");
    let cleanup = portable_delete_objects(&device.id, &[folder_id]);
    let _ = std::fs::remove_file(local_file);
    let _ = std::fs::remove_file(downloaded_image);
    if cleanup.is_ok() {
        let _ = std::fs::remove_file(fixture_path);
    }
    cleanup.expect("remove test folder");
    result.expect("deleting and refreshing the phone must finish successfully");
    eprintln!("Reopening a fresh connection after deletion");
    assert!(
        !portable_device_objects_result(&device.id, "DEVICE")
            .expect("reopen phone")
            .is_empty()
    );
    eprintln!("Phone regression test completed in {:?}", started.elapsed());
}
