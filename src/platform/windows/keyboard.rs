use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_MENU,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_SHIFT, VK_V,
};

pub fn file_paste_shortcut_down() -> bool {
    control_down() && key_down(VK_V) && !alt_down() && !shift_down()
}

fn control_down() -> bool {
    key_down(VK_CONTROL) || key_down(VK_LCONTROL) || key_down(VK_RCONTROL)
}

fn alt_down() -> bool {
    key_down(VK_MENU) || key_down(VK_LMENU) || key_down(VK_RMENU)
}

fn shift_down() -> bool {
    key_down(VK_SHIFT) || key_down(VK_LSHIFT) || key_down(VK_RSHIFT)
}

fn key_down(key: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(key.0 as i32) as u16 & 0x8000 != 0 }
}
