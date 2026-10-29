//! The Windows PC settings host (Story 6.35).
//!
//! The service is LocalSystem, but the pointer speed, the display mode, the sound output and the
//! keyboard layout belong to the player's interactive session. So every call runs a short-lived
//! helper, `arena-kiosk-svc pc-settings read | mouse N | display WxH@Hz | audio HEX | layout ID`,
//! started in the player's session the way the cleanup's helper is
//! ([`cleanup_windows`](crate::cleanup_windows)); `read` prints the PC's settings and choices as
//! JSON on its standard output.
//!
//! - **Mouse:** `SystemParametersInfo(SPI_SETMOUSESPEED)` for this session only (it is not
//!   written to the player's profile).
//! - **Display:** `EnumDisplaySettings` lists the modes the primary monitor supports;
//!   `ChangeDisplaySettingsEx` with no flags switches to one dynamically (it is not written to the
//!   registry, so it does not outlive the session if the cleanup never ran).
//! - **Sound output:** the active render endpoints are listed by name; the default is set for the
//!   player's account through the system's policy-config interface (all three roles).
//! - **Keyboard layout:** the layouts installed for the player are listed by `GetKeyboardLayoutList`
//!   (a layout's id is its handle as 8 hex digits); one is activated for the session's default
//!   input language and for the window in front.

use std::path::PathBuf;

use arena_proto::PcLayout;

use crate::cleanup_windows::run_as_capture;
use crate::cli::{PcTask, encode_hex_name};
use crate::launcher::console_player_token;
use crate::pc_settings::{PcSettingsHost, PcSnapshot};

/// The real PC.
pub struct WinPcSettingsHost {
    config: PathBuf,
}

impl WinPcSettingsHost {
    /// A host for the player account named in the config at `config`.
    pub fn new(config: PathBuf) -> Self {
        Self { config }
    }

    fn player(&self) -> String {
        crate::config::KioskConfig::load(&self.config)
            .ok()
            .flatten()
            .map(|c| c.player_user)
            .unwrap_or_else(|| crate::config::DEFAULT_PLAYER_USER.to_owned())
    }

    /// Runs the helper with `args` in the player's session. `None` when nobody is signed in as
    /// the player (there is nothing to change then).
    fn helper(&self, args: &str, capture: bool) -> Result<Option<Vec<u8>>, String> {
        let token = match console_player_token(&self.player()) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, "no player session; the PC settings helper is skipped");
                return Ok(None);
            }
        };
        let exe = std::env::current_exe().map_err(|e| format!("no service path: {e}"))?;
        let cmd = format!("\"{}\" pc-settings {args}", exe.display());
        run_as_capture(
            &token,
            &cmd,
            exe.parent().map(|d| d.display().to_string()),
            capture,
        )
        .map(Some)
    }
}

impl PcSettingsHost for WinPcSettingsHost {
    fn snapshot(&self) -> Result<PcSnapshot, String> {
        let out = self
            .helper("read", true)?
            .ok_or_else(|| "nobody is signed in as the player".to_owned())?;
        serde_json::from_slice(&out).map_err(|e| format!("the PC settings were unreadable: {e}"))
    }

    fn set_mouse_speed(&self, speed: u8) -> Result<(), String> {
        self.helper(&format!("mouse {speed}"), false).map(|_| ())
    }

    fn set_display_mode(&self, mode: &str) -> Result<(), String> {
        self.helper(&format!("display {mode}"), false).map(|_| ())
    }

    fn set_audio_output(&self, name: &str) -> Result<(), String> {
        self.helper(&format!("audio {}", encode_hex_name(name)), false)
            .map(|_| ())
    }

    fn set_keyboard_layout(&self, id: &str) -> Result<(), String> {
        self.helper(&format!("layout {id}"), false).map(|_| ())
    }
}

/// `arena-kiosk-svc pc-settings ...`: runs in the player's session.
pub fn run_helper(task: PcTask) -> Result<(), String> {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    // SAFETY: balanced with `CoUninitialize` below.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if com.is_err() {
        return Err(format!("COM did not start: {com:?}"));
    }
    let result = match task {
        PcTask::Read => read().and_then(|snap| {
            let json = serde_json::to_string(&snap).map_err(|e| e.to_string())?;
            println!("{json}");
            Ok(())
        }),
        PcTask::Mouse(speed) => set_mouse_speed(speed),
        PcTask::Display(mode) => set_display_mode(&mode),
        PcTask::Audio(name) => set_audio_output(&name),
        PcTask::Layout(id) => set_keyboard_layout(&id),
    };
    // SAFETY: COM was initialised on this thread above.
    unsafe { CoUninitialize() };
    result
}

/// Everything the panel needs, read in the player's session. A part this PC cannot read (no
/// sound device, one keyboard layout the list cannot name) is empty rather than an error.
fn read() -> Result<PcSnapshot, String> {
    let (display_mode, display_modes) = displays()?;
    let (audio_outputs, audio_output) = match audio() {
        Ok((all, default)) => (all.into_iter().map(|d| d.name).collect(), default),
        Err(e) => {
            eprintln!("no sound outputs: {e}");
            (Vec::new(), None)
        }
    };
    let (keyboard_layouts, keyboard_layout) = layouts();
    Ok(PcSnapshot {
        mouse_speed: mouse_speed()?,
        display_mode,
        display_modes,
        audio_outputs,
        audio_output,
        keyboard_layouts,
        keyboard_layout,
    })
}

// ---- Mouse ----

fn mouse_speed() -> Result<u8, String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETMOUSESPEED, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut speed = 0u32;
    // SAFETY: `speed` is the `UINT` this action writes.
    unsafe {
        SystemParametersInfoW(
            SPI_GETMOUSESPEED,
            0,
            Some((&mut speed as *mut u32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .map_err(|e| format!("the mouse speed cannot be read: {e}"))?;
    u8::try_from(speed)
        .ok()
        .filter(|s| arena_proto::valid_mouse_speed(*s))
        .ok_or_else(|| format!("the mouse speed {speed} is outside 1 to 20"))
}

fn set_mouse_speed(speed: u8) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_SETMOUSESPEED, SPIF_SENDCHANGE, SystemParametersInfoW,
    };
    if !arena_proto::valid_mouse_speed(speed) {
        return Err("the mouse speed is 1 to 20".into());
    }
    // SAFETY: for this action the value travels in the pointer argument itself.
    unsafe {
        SystemParametersInfoW(
            SPI_SETMOUSESPEED,
            0,
            Some(usize::from(speed) as *mut core::ffi::c_void),
            SPIF_SENDCHANGE,
        )
    }
    .map_err(|e| format!("the mouse speed did not set: {e}"))
}

// ---- Display ----

fn blank_mode() -> windows::Win32::Graphics::Gdi::DEVMODEW {
    windows::Win32::Graphics::Gdi::DEVMODEW {
        dmSize: std::mem::size_of::<windows::Win32::Graphics::Gdi::DEVMODEW>() as u16,
        ..Default::default()
    }
}

fn spelled(dm: &windows::Win32::Graphics::Gdi::DEVMODEW) -> String {
    format!(
        "{}x{}@{}",
        dm.dmPelsWidth, dm.dmPelsHeight, dm.dmDisplayFrequency
    )
}

/// The primary monitor's current mode and every mode it supports at the current colour depth.
fn displays() -> Result<(String, Vec<String>), String> {
    use windows::Win32::Graphics::Gdi::{
        ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE, EnumDisplaySettingsW,
    };
    use windows::core::PCWSTR;
    let mut current = blank_mode();
    // SAFETY: `current` is a DEVMODEW with its size set.
    if !unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut current) }
        .as_bool()
    {
        return Err("the display mode cannot be read".into());
    }
    let mut modes: Vec<(u32, u32, u32)> = Vec::new();
    for i in 0..4096u32 {
        let mut dm = blank_mode();
        // SAFETY: as above.
        if !unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_DISPLAY_SETTINGS_MODE(i), &mut dm) }
            .as_bool()
        {
            break;
        }
        if dm.dmBitsPerPel == current.dmBitsPerPel
            && arena_proto::parse_display_mode(&spelled(&dm)).is_some()
        {
            modes.push((dm.dmPelsWidth, dm.dmPelsHeight, dm.dmDisplayFrequency));
        }
    }
    modes.sort_unstable();
    modes.dedup();
    Ok((
        spelled(&current),
        modes
            .into_iter()
            .map(|(w, h, hz)| format!("{w}x{h}@{hz}"))
            .collect(),
    ))
}

fn set_display_mode(mode: &str) -> Result<(), String> {
    use windows::Win32::Graphics::Gdi::{
        CDS_TEST, CDS_TYPE, ChangeDisplaySettingsExW, DEVMODE_FIELD_FLAGS, DISP_CHANGE_SUCCESSFUL,
        DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH, ENUM_CURRENT_SETTINGS,
        EnumDisplaySettingsW,
    };
    use windows::core::PCWSTR;
    let wanted =
        arena_proto::parse_display_mode(mode).ok_or_else(|| "not a display mode".to_owned())?;
    let mut dm = blank_mode();
    // SAFETY: `dm` is a DEVMODEW with its size set.
    if !unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut dm) }.as_bool() {
        return Err("the display mode cannot be read".into());
    }
    dm.dmPelsWidth = wanted.width;
    dm.dmPelsHeight = wanted.height;
    dm.dmDisplayFrequency = wanted.hz;
    dm.dmFields = DEVMODE_FIELD_FLAGS(DM_PELSWIDTH.0 | DM_PELSHEIGHT.0 | DM_DISPLAYFREQUENCY.0);
    // SAFETY: `dm` is a complete DEVMODEW; the primary display is named by a null device.
    let tested =
        unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), Some(&dm), None, CDS_TEST, None) };
    if tested != DISP_CHANGE_SUCCESSFUL {
        return Err(format!("the monitor refused the mode (code {})", tested.0));
    }
    // No flags: a dynamic change, not written to the registry.
    // SAFETY: as above.
    let done =
        unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), Some(&dm), None, CDS_TYPE(0), None) };
    if done == DISP_CHANGE_SUCCESSFUL {
        Ok(())
    } else {
        Err(format!("the display mode did not change (code {})", done.0))
    }
}

// ---- Sound output ----

/// A render endpoint: its system id and its name.
struct Endpoint {
    id: String,
    name: String,
}

fn from_pwstr(p: windows::core::PWSTR) -> String {
    // SAFETY: `p` is a NUL-terminated string from the audio service, freed below.
    let text = unsafe { p.to_string() }.unwrap_or_default();
    // SAFETY: allocated by the audio service with `CoTaskMemAlloc`.
    unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(p.0.cast())) };
    text
}

fn friendly_name(device: &windows::Win32::Media::Audio::IMMDevice) -> Result<String, String> {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::System::Com::STGM_READ;
    use windows::Win32::System::Com::StructuredStorage::{
        PropVariantClear, PropVariantToStringAlloc,
    };
    // SAFETY: COM calls on interfaces owned by this function.
    unsafe {
        let store = device
            .OpenPropertyStore(STGM_READ)
            .map_err(|e| format!("no device properties: {e}"))?;
        let mut value = store
            .GetValue(&PKEY_Device_FriendlyName)
            .map_err(|e| format!("no device name: {e}"))?;
        let name = PropVariantToStringAlloc(&value);
        let _ = PropVariantClear(&mut value);
        Ok(from_pwstr(
            name.map_err(|e| format!("no device name: {e}"))?,
        ))
    }
}

/// The active render endpoints, and the name of the default one.
fn audio() -> Result<(Vec<Endpoint>, Option<String>), String> {
    use windows::Win32::Media::Audio::{
        DEVICE_STATE_ACTIVE, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
    // SAFETY: COM calls on interfaces owned by this function.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("no audio service: {e}"))?;
        let all = enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("no output devices: {e}"))?;
        let mut out = Vec::new();
        for i in 0..all.GetCount().map_err(|e| e.to_string())? {
            let device = all.Item(i).map_err(|e| e.to_string())?;
            let id = from_pwstr(device.GetId().map_err(|e| e.to_string())?);
            if let Ok(name) = friendly_name(&device) {
                out.push(Endpoint { id, name });
            }
        }
        let default = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .ok()
            .and_then(|d| d.GetId().ok())
            .map(from_pwstr)
            .and_then(|id| out.iter().find(|e| e.id == id).map(|e| e.name.clone()));
        Ok((out, default))
    }
}

/// The system's policy-config interface, the only way to make a device the default for the
/// account (it is not documented; Windows' own Sound settings use it). Only `SetDefaultEndpoint`
/// is called, so the table spells the methods before it as plain slots.
#[repr(C)]
struct PolicyConfigVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
    /// `GetMixFormat` to `SetPropertyValue`.
    unused: [usize; 10],
    set_default_endpoint: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        windows::core::PCWSTR,
        windows::Win32::Media::Audio::ERole,
    ) -> windows::core::HRESULT,
}

/// `CLSID_PolicyConfigClient` and `IID_IPolicyConfig` (the Windows 10 and 11 interface).
const POLICY_CONFIG_CLIENT: windows::core::GUID =
    windows::core::GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
const POLICY_CONFIG: windows::core::GUID =
    windows::core::GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);

fn set_audio_output(name: &str) -> Result<(), String> {
    use windows::Win32::Media::Audio::{eCommunications, eConsole, eMultimedia};
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
    use windows::core::{IUnknown, Interface};
    let (all, _) = audio()?;
    let target = all
        .iter()
        .find(|e| e.name == name)
        .ok_or_else(|| "no such sound output".to_owned())?;
    let id: Vec<u16> = target.id.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: COM calls; `id` is NUL-terminated and outlives the calls, and the interface
    // pointer is released on every path.
    unsafe {
        let unknown: IUnknown = CoCreateInstance(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
            .map_err(|e| format!("no sound policy service: {e}"))?;
        let mut policy: *mut core::ffi::c_void = std::ptr::null_mut();
        unknown
            .query(&POLICY_CONFIG, &mut policy)
            .ok()
            .map_err(|e| format!("no sound policy service: {e}"))?;
        let vtbl = &**(policy as *mut *const PolicyConfigVtbl);
        let mut result = Ok(());
        for role in [eConsole, eMultimedia, eCommunications] {
            let hr = (vtbl.set_default_endpoint)(policy, windows::core::PCWSTR(id.as_ptr()), role);
            if let Err(e) = hr.ok() {
                result = Err(format!("the sound output did not change: {e}"));
                break;
            }
        }
        (vtbl.release)(policy);
        result
    }
}

// ---- Keyboard layout ----

/// The layouts installed for the player, and the active one in front: ids are the layout
/// handles as 8 hex digits.
fn layouts() -> (Vec<PcLayout>, Option<String>) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, GetKeyboardLayoutList};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    // SAFETY: plain Win32 calls with buffers owned here.
    unsafe {
        let count = GetKeyboardLayoutList(None);
        let mut list = vec![Default::default(); usize::try_from(count).unwrap_or(0)];
        let got = GetKeyboardLayoutList(Some(&mut list));
        list.truncate(usize::try_from(got).unwrap_or(0));
        let front = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let active = GetKeyboardLayout(front);
        let layouts = list
            .iter()
            .map(|hkl| PcLayout {
                id: layout_id(hkl.0 as usize as u32),
                name: layout_name(hkl.0 as usize as u32),
            })
            .collect();
        (layouts, Some(layout_id(active.0 as usize as u32)))
    }
}

fn layout_id(hkl: u32) -> String {
    format!("{hkl:08X}")
}

/// What the player sees: the language ("English (United States)"), and for a layout that is not
/// the language's default one its layout number.
fn layout_name(hkl: u32) -> String {
    use windows::Win32::Globalization::{GetLocaleInfoW, LOCALE_SLOCALIZEDDISPLAYNAME};
    let lang = hkl & 0xFFFF;
    let mut buf = [0u16; 128];
    // SAFETY: `buf` is the buffer handed in.
    let n = unsafe { GetLocaleInfoW(lang, LOCALE_SLOCALIZEDDISPLAYNAME, Some(&mut buf)) };
    let name = if n > 1 {
        String::from_utf16_lossy(&buf[..(n as usize - 1)])
    } else {
        format!("Keyboard {lang:04X}")
    };
    if hkl >> 16 == lang {
        name
    } else {
        format!("{name} ({:04X})", hkl >> 16)
    }
}

fn set_keyboard_layout(id: &str) -> Result<(), String> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayoutList, HKL};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, PostMessageW, SPI_SETDEFAULTINPUTLANG, SPIF_SENDCHANGE,
        SystemParametersInfoW, WM_INPUTLANGCHANGEREQUEST,
    };
    if !arena_proto::valid_keyboard_layout(id) {
        return Err("not a keyboard layout id".into());
    }
    // SAFETY: plain Win32 calls with buffers owned here.
    unsafe {
        let count = GetKeyboardLayoutList(None);
        let mut list = vec![HKL::default(); usize::try_from(count).unwrap_or(0)];
        let got = GetKeyboardLayoutList(Some(&mut list));
        list.truncate(usize::try_from(got).unwrap_or(0));
        let hkl = list
            .into_iter()
            .find(|h| layout_id(h.0 as usize as u32) == id)
            .ok_or_else(|| "that keyboard layout is not installed".to_owned())?;
        // New windows start on it; the window in front switches now.
        let mut value = hkl;
        SystemParametersInfoW(
            SPI_SETDEFAULTINPUTLANG,
            0,
            Some((&mut value as *mut HKL).cast()),
            SPIF_SENDCHANGE,
        )
        .map_err(|e| format!("the default input language did not change: {e}"))?;
        let front = GetForegroundWindow();
        if !front.is_invalid() {
            PostMessageW(
                Some(front),
                WM_INPUTLANGCHANGEREQUEST,
                WPARAM(0),
                LPARAM(hkl.0 as isize),
            )
            .map_err(|e| format!("the window in front did not switch: {e}"))?;
        }
    }
    Ok(())
}
