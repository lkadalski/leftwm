use std::os::raw::c_ulong;

use super::{models::Screen, models::Window, models::WindowHandle, Button, ModMask, XKeysym};
use crate::models::WindowChange;
use crate::Command;

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum DisplayEvent {
    Movement(WindowHandle, i32, i32),
    KeyCombo(ModMask, XKeysym),
    KeyGrabReload, // Reloads keys for when keyboard changes
    MouseCombo(ModMask, Button, WindowHandle),
    WindowCreate(Window, i32, i32),
    WindowChange(WindowChange),
    WindowDestroy(WindowHandle),
    MouseEnteredWindow(WindowHandle),
    VerifyFocusedAt(i32, i32), //Request focus validation at this point
    MoveFocusTo(i32, i32),     //Focus the nearest window to this point
    MoveWindow(WindowHandle, c_ulong, i32, i32),
    ResizeWindow(WindowHandle, c_ulong, i32, i32),
    ScreenCreate(Screen),
    SendCommand(Command, Option<String>),
    ChangeToNormalMode,
}
