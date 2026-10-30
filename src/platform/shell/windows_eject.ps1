# Only function definitions live here so discovery can be checked without
# calling the operation that prepares a physical device for removal.
function Get-BExplorerPnpRemovalTarget([string]$InstanceId) {
    $seen = @{}
    for ($depth = 0; $depth -lt 16; $depth++) {
        if ([string]::IsNullOrWhiteSpace($InstanceId) -or $seen.ContainsKey($InstanceId)) {
            throw 'Could not resolve a removable device without crossing the USB controller'
        }
        $seen[$InstanceId] = $true
        $properties = @(Get-PnpDeviceProperty -InstanceId $InstanceId -KeyName @(
            'DEVPKEY_Device_Capabilities', 'DEVPKEY_Device_Parent', 'DEVPKEY_Device_Service'
        ) -ErrorAction Stop)
        $capabilityProperty = $properties | Where-Object KeyName -eq 'DEVPKEY_Device_Capabilities' | Select-Object -First 1
        $parentProperty = $properties | Where-Object KeyName -eq 'DEVPKEY_Device_Parent' | Select-Object -First 1
        $serviceProperty = $properties | Where-Object KeyName -eq 'DEVPKEY_Device_Service' | Select-Object -First 1
        $service = [string]$serviceProperty.Data
        # A USB enclosure can have a removable parent while its disk child
        # reports fixed media. Never continue up into a hub or host controller.
        if ($service -match '^(?i:usbhub\d*|usbxhci|usbehci|usbohci|usbuhci|usbport)$' -or
            $InstanceId -match '^(?i:USB\\ROOT_|PCI\\|ROOT\\|HTREE\\)') {
            throw 'Refusing to remove a USB hub or host controller'
        }
        if ($null -ne $capabilityProperty.Data -and ([uint32]$capabilityProperty.Data -band 4) -ne 0) {
            return $InstanceId
        }
        $InstanceId = [string]$parentProperty.Data
    }
    throw 'The removable-device parent chain is too deep'
}

function Resolve-BExplorerUsbRemovalTarget([string]$DriveRoot) {
    if ($DriveRoot -notmatch '^[A-Za-z]:\\$') { throw 'A drive-letter root is required for safe removal' }
    $disks = @(Get-Partition -DriveLetter $DriveRoot.Substring(0, 1) -ErrorAction Stop | Get-Disk -ErrorAction Stop)
    if ($disks.Count -ne 1) { throw 'The volume must resolve to exactly one physical disk' }
    $disk = $disks[0]
    if ($null -eq $disk.IsBoot -or $null -eq $disk.IsSystem -or $disk.IsBoot -or $disk.IsSystem) {
        throw 'Refusing to remove a boot or system disk'
    }
    if ([string]$disk.BusType -notin @('USB', '1394')) {
        throw 'This volume is not on an external USB or IEEE 1394 disk'
    }
    $physical = @(Get-CimInstance Win32_DiskDrive -Filter ('Index = {0}' -f [uint32]$disk.Number) -ErrorAction Stop)
    if ($physical.Count -ne 1 -or [string]::IsNullOrWhiteSpace([string]$physical[0].PNPDeviceID)) {
        throw 'Could not locate the physical disk device instance'
    }
    Get-BExplorerPnpRemovalTarget ([string]$physical[0].PNPDeviceID)
}

function Initialize-BExplorerDeviceRemoval {
    if ('BExplorerDeviceRemoval' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class BExplorerDeviceRemoval {
    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern uint CM_Locate_DevNodeW(out uint instance, string id, uint flags);
    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern uint CM_Request_Device_EjectW(uint instance, out uint vetoType,
        StringBuilder vetoName, uint length, uint flags);
    public static string DescribeFailure(uint result, uint veto, string name) {
        string marker = result == 23 && (veto == 2 || veto == 3 || veto == 4 || veto == 5)
            ? "BEXPLORER_EJECT_BUSY" : "BEXPLORER_EJECT_FAILED";
        // OutstandingOpen names the child device with open handles; it does
        // not mean that the child was selected as the removal target.
        // Keep diagnostics separate from the localized user-facing message.
        return marker + "|PnP=" + result + "|Veto=" + veto + "|Device=" + name;
    }
    public static uint Request(string id, out uint veto, out string vetoName) {
        veto = 0;
        vetoName = "";
        uint instance;
        // Resolve the original identity again on every bounded retry. A drive
        // letter reassigned to another disk must never redirect the request.
        uint result = CM_Locate_DevNodeW(out instance, id, 0);
        if (result != 0) throw new InvalidOperationException("Device is no longer present (PnP " + result + ")");
        var name = new StringBuilder(260);
        result = CM_Request_Device_EjectW(instance, out veto, name, 260, 0);
        vetoName = name.ToString();
        return result;
    }
}
'@ -ErrorAction Stop
}

function Invoke-BExplorerPnpRemovalAttempt([string]$InstanceId) {
    Initialize-BExplorerDeviceRemoval
    $veto = [uint32]0
    $name = [string]''
    $result = [BExplorerDeviceRemoval]::Request($InstanceId, [ref]$veto, [ref]$name)
    [pscustomobject]@{ Result = $result; Veto = $veto; Name = $name }
}

function Request-BExplorerPnpRemoval([string]$InstanceId) {
    for ($attempt = 0; $attempt -lt 3; $attempt++) {
        $outcome = Invoke-BExplorerPnpRemovalAttempt $InstanceId
        if ($outcome.Result -eq 0) { return }
        # A metadata provider or background reader can take a moment to close
        # its handles. Only OutstandingOpen is retried, through the same safe
        # Windows request; other vetoes are returned immediately.
        if ($outcome.Result -eq 23 -and $outcome.Veto -eq 5 -and $attempt -lt 2) {
            Start-Sleep -Milliseconds 300
        } else {
            throw [BExplorerDeviceRemoval]::DescribeFailure($outcome.Result, $outcome.Veto, $outcome.Name)
        }
    }
}

function Test-BExplorerDrivePresent([string]$DriveRoot) {
    # Enumerate assigned letters instead of opening the root: access errors
    # or a temporarily stalled file system do not mean removal succeeded.
    return @([System.IO.DriveInfo]::GetDrives() | Where-Object Name -eq $DriveRoot).Count -gt 0
}

function Get-BExplorerDriveInfo([string]$DriveRoot) {
    return [System.IO.DriveInfo]::new($DriveRoot)
}

function Get-BExplorerOpticalMediaReady([string]$DriveRoot) {
    try {
        $drive = Get-BExplorerDriveInfo $DriveRoot
        if ($drive.DriveType -ne [System.IO.DriveType]::CDRom) { return $null }
        return [bool]$drive.IsReady
    } catch {
        # Disappearance is checked separately. A property-access error must
        # not be mistaken for successful media removal on a USB disk.
        return $null
    }
}

function Release-BExplorerComObject([object]$Object) {
    if ($null -ne $Object -and [System.Runtime.InteropServices.Marshal]::IsComObject($Object)) {
        [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object)
    }
}

function Write-BExplorerEjectError([System.Exception]$Exception) {
    # PowerShell wraps native/.NET failures in MethodInvocationException.
    # Report the underlying message, without a localized invocation wrapper.
    while ($null -ne $Exception.InnerException) { $Exception = $Exception.InnerException }
    [Console]::Error.WriteLine($Exception.Message)
}

function Invoke-BExplorerDriveEject([string]$DriveRoot) {
    $ErrorActionPreference = 'Stop'
    if ($DriveRoot -notmatch '^[A-Za-z]:\\$') { throw 'A drive-letter root is required for safe removal' }
    $shell = $folder = $item = $verbs = $ejectVerb = $null
    $allVerbs = @()
    $needsPnpRemoval = $false
    $hadOpticalMedia = $false
    try {
        $shell = New-Object -ComObject Shell.Application
        $folder = $shell.Namespace(17)
        $item = $folder.ParseName($DriveRoot.TrimEnd([char]92))
        if (-not $item) { $item = $folder.ParseName($DriveRoot) }
        if (-not $item) { throw "Drive not found: $DriveRoot" }

        $verbs = $item.Verbs()
        $allVerbs = @($verbs)
        $ejectVerb = $allVerbs | Where-Object {
            $name = (([string]$_.Name).Replace('&', '').Replace([string][char]0x2026, '').Trim())
            $name -match '^(?i:eject|expulsar|quitar|safely remove)$'
        } | Select-Object -First 1
        if ($ejectVerb) {
            $opticalReady = Get-BExplorerOpticalMediaReady $DriveRoot
            $hadOpticalMedia = $null -ne $opticalReady -and $opticalReady -eq $true
            $ejectVerb.DoIt()
        } else {
            $needsPnpRemoval = $true
        }
    } finally {
        # Do not retain Shell folder/item references while asking Windows to
        # remove their volume, or until the child PowerShell process exits.
        foreach ($verb in $allVerbs) { Release-BExplorerComObject $verb }
        Release-BExplorerComObject $verbs
        Release-BExplorerComObject $item
        Release-BExplorerComObject $folder
        Release-BExplorerComObject $shell
    }
    if ($needsPnpRemoval) {
        # USB fixed-media disks often expose removal only through Windows'
        # safely-remove tray, without an Eject verb on their drive-letter item.
        $target = Resolve-BExplorerUsbRemovalTarget $DriveRoot
        Request-BExplorerPnpRemoval $target
    }

    # USB/PnP removal must remove the assigned letter. Optical drives retain
    # that letter after ejecting a disc, so accept a verified ready-to-empty
    # transition only for media that was present in a CD/DVD drive beforehand.
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        Start-Sleep -Milliseconds 250
        if (-not (Test-BExplorerDrivePresent $DriveRoot)) { return }
        if ($hadOpticalMedia) {
            $opticalReady = Get-BExplorerOpticalMediaReady $DriveRoot
            if ($null -ne $opticalReady -and $opticalReady -eq $false) { return }
        }
    }
    throw "Windows did not remove $DriveRoot after requesting safe removal"
}
