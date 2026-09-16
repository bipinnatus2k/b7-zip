#![allow(non_snake_case)]

//! Reusable engine for Windows 11 native (top-level) Explorer context menus.
//!
//! Built on a sparse MSIX package that binds root CLSIDs to
//! `windows.fileExplorerContextMenus` verbs, plus an in-process COM server
//! implementing `IExplorerCommand`. Hosts describe their menu entries as
//! [`MenuAction`] implementations and register them through [`init`]:
//!
//! * [`MenuRoot`] entries appear as flyout menus; their children are produced
//!   by a builder on every enumeration so they can follow live app state.
//! * [`StandaloneCommand`] entries are activatable directly through their own
//!   CLSID, which supports classic `DelegateExecute`/`CommandStore`
//!   registrations, per-verb manifest entries, and programmatic invocation.
//!
//! Hosts forward the standard DLL exports to [`dll_get_class_object`] and
//! [`dll_can_unload_now`].

use std::cell::Cell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use windows::core::{implement, Interface, GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_NOTIMPL, E_OUTOFMEMORY,
    E_POINTER, HMODULE, HWND, S_FALSE,
};
use windows::Win32::System::Com::{
    CoTaskMemAlloc, CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl,
};
use windows::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW};
use windows::Win32::UI::Shell::{
    IEnumExplorerCommand, IEnumExplorerCommand_Impl, IExplorerCommand, IExplorerCommand_Impl,
    IShellItemArray, ECF_DEFAULT, ECF_HASSUBCOMMANDS, ECS_DISABLED, ECS_ENABLED, ECS_HIDDEN,
    SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

pub use windows::core::HRESULT;
pub use windows::Win32::Foundation::{BOOL, HINSTANCE};

/// Visibility/state of a menu entry as reported to Explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandState {
    Enabled,
    Disabled,
    Hidden,
}

impl CommandState {
    fn to_raw(self) -> u32 {
        match self {
            CommandState::Enabled => ECS_ENABLED.0 as u32,
            CommandState::Disabled => ECS_DISABLED.0 as u32,
            CommandState::Hidden => ECS_HIDDEN.0 as u32,
        }
    }
}

/// The items the context menu was opened on.
///
/// `None` means Explorer provided no item array or paths could not be read;
/// `Some` always contains filesystem paths (`SIGDN_FILESYSPATH`).
#[derive(Debug, Clone, Default)]
pub struct Selection {
    paths: Vec<PathBuf>,
}

impl Selection {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn first(&self) -> Option<&Path> {
        self.paths.first().map(PathBuf::as_path)
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// True when there is at least one path and every path is an existing directory.
    pub fn all_dirs(&self) -> bool {
        !self.paths.is_empty() && self.paths.iter().all(|p| p.is_dir())
    }

    /// True when there is at least one path and every path is an existing file.
    pub fn all_files(&self) -> bool {
        !self.paths.is_empty() && self.paths.iter().all(|p| p.is_file())
    }
}

/// A single executable entry inside a [`MenuRoot`] flyout.
pub trait MenuAction: Send + Sync {
    /// Display label shown by Explorer.
    fn title(&self) -> String;

    /// Hover tooltip; defaults to [`MenuAction::title`].
    fn tooltip(&self) -> Option<String> {
        None
    }

    /// Icon path; relative paths resolve against the host DLL's directory.
    fn icon(&self) -> Option<PathBuf> {
        None
    }

    /// Canonical verb GUID reported via `IExplorerCommand::GetCanonicalName`.
    fn canonical_name(&self) -> Option<GUID> {
        None
    }

    /// Called whenever Explorer queries the entry state for a selection.
    fn state(&self, _selection: Option<&Selection>) -> CommandState {
        CommandState::Enabled
    }

    /// Runs the command. Errors are surfaced in a message box and mapped to `E_FAIL`.
    fn invoke(&self, selection: Option<&Selection>) -> Result<(), String>;
}

type ActionBuilder = dyn Fn() -> Vec<Arc<dyn MenuAction>> + Send + Sync;

/// A top-level flyout registered under one CLSID in the sparse package manifest.
///
/// `build_actions` runs on every enumeration so entries can be recomposed from
/// live application state; per-entry visibility can additionally be decided in
/// [`MenuAction::state`].
pub struct MenuRoot {
    pub clsid: GUID,
    pub title: String,
    pub tooltip: Option<String>,
    pub icon: Option<PathBuf>,
    pub build_actions: Box<ActionBuilder>,
}

impl MenuRoot {
    pub fn new(clsid: GUID, title: impl Into<String>, build_actions: Box<ActionBuilder>) -> Self {
        Self {
            clsid,
            title: title.into(),
            tooltip: None,
            icon: None,
            build_actions,
        }
    }
}

/// A menu entry that COM clients can activate directly through
/// `CoCreateInstance` on `clsid`, without enumerating a parent root.
///
/// Typical consumers are classic `shell\...\command\DelegateExecute` keys,
/// `CommandStore` verbs, per-verb sparse-package manifests, and automated
/// tests that drive commands headlessly.
pub struct StandaloneCommand {
    pub clsid: GUID,
    pub action: Arc<dyn MenuAction>,
}

impl StandaloneCommand {
    pub fn new(clsid: GUID, action: Arc<dyn MenuAction>) -> Self {
        Self { clsid, action }
    }
}

/// The set of COM classes exposed by the host DLL.
pub struct Registration {
    roots: Vec<Arc<MenuRoot>>,
    standalone: Vec<StandaloneCommand>,
}

impl Registration {
    pub fn new(roots: Vec<MenuRoot>, standalone: Vec<StandaloneCommand>) -> Self {
        Self {
            roots: roots.into_iter().map(Arc::new).collect(),
            standalone,
        }
    }

    fn factory_for(&self, clsid: &GUID) -> Option<IClassFactory> {
        if let Some(root) = self.roots.iter().find(|root| root.clsid == *clsid) {
            return Some(
                ClassFactory {
                    content: FactoryContent::Root(Arc::clone(root)),
                }
                .into(),
            );
        }
        self.standalone
            .iter()
            .find(|command| command.clsid == *clsid)
            .map(|command| {
                ClassFactory {
                    content: FactoryContent::Action(Arc::clone(&command.action)),
                }
                .into()
            })
    }
}

static REGISTRATION: OnceLock<Registration> = OnceLock::new();
static MODULE_PATH: OnceLock<PathBuf> = OnceLock::new();

type ErrorReporter = dyn Fn(&str, &str) + Send + Sync;

static ERROR_REPORTER: RwLock<Option<Box<ErrorReporter>>> = RwLock::new(None);

/// Replaces the default modal `MessageBoxW` used to surface [`MenuAction::invoke`]
/// failures. Pass `None` to restore the default. The callback must not call
/// back into `set_error_reporter`.
pub fn set_error_reporter(reporter: Option<Box<ErrorReporter>>) {
    let mut slot = ERROR_REPORTER
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = reporter;
}

/// Installs the menu definitions and records the host module path.
///
/// Call once from `DllMain` with `DLL_PROCESS_ATTACH`. Returns false when a
/// previous call already installed a different registration.
pub fn init(module: HINSTANCE, registration: Registration) -> bool {
    let _ = unsafe { DisableThreadLibraryCalls(module) };
    if let Some(path) = module_path(module) {
        let _ = MODULE_PATH.set(path);
    }
    REGISTRATION.set(registration).is_ok()
}

/// Implementation of the host DLL's `DllGetClassObject` export.
///
/// Roots are looked up first, then standalone commands; unknown CLSIDs yield
/// `CLASS_E_CLASSNOTAVAILABLE`.
///
/// # Safety
/// Pointers must originate from the unmodified COM activation call.
pub unsafe fn dll_get_class_object(
    rclsid: *const GUID,
    riid: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || object.is_null() {
        return E_POINTER;
    }
    *object = std::ptr::null_mut();

    let registration = match REGISTRATION.get() {
        Some(registration) => registration,
        None => return CLASS_E_CLASSNOTAVAILABLE,
    };

    match registration.factory_for(&*rclsid) {
        Some(factory) => factory.query(riid, object).ok().into(),
        None => CLASS_E_CLASSNOTAVAILABLE,
    }
}

/// Implementation of the host DLL's `DllCanUnloadNow` export.
///
/// Always reports the DLL as unloadable-in-principle but keeps it loaded,
/// which avoids reload churn inside Explorer.
pub fn dll_can_unload_now() -> HRESULT {
    S_FALSE
}

/// Directory containing the host DLL, if `DllMain` ran.
pub fn module_directory() -> Option<&'static Path> {
    MODULE_PATH.get().map(PathBuf::as_path)
}

/// Shows an error through the active reporter (modal message box by default).
pub fn show_error_box(title: &str, message: &str) {
    let reporter = ERROR_REPORTER
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match reporter.as_ref() {
        Some(reporter) => reporter(title, message),
        None => show_message_box(title, message),
    }
}

fn show_message_box(title: &str, message: &str) {
    let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(
            HWND(std::ptr::null_mut()),
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn resolve_icon(base: Option<&Path>, icon: Option<&Path>) -> Option<PathBuf> {
    match icon {
        Some(icon) if icon.is_absolute() => Some(icon.to_path_buf()),
        Some(icon) => base.map(|base| base.join(icon)),
        None => None,
    }
}

fn module_path(module: HINSTANCE) -> Option<PathBuf> {
    let mut buffer = [0u16; 32768];
    let module = HMODULE(module.0);
    let len = unsafe { GetModuleFileNameW(Some(&module), &mut buffer) };
    if len == 0 {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(
        &buffer[..len as usize],
    )))
}

fn extract_selection(items: Option<&IShellItemArray>) -> Result<Option<Selection>, String> {
    let items = match items {
        Some(items) => items,
        None => return Ok(None),
    };
    let count = unsafe { items.GetCount() }.map_err(|err| err.message().to_string())?;
    let mut paths = Vec::with_capacity(count as usize);
    for index in 0..count {
        let item =
            unsafe { items.GetItemAt(index) }.map_err(|err| err.message().to_string())?;
        let raw = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }
            .map_err(|err| err.message().to_string())?;
        let path = unsafe { raw.to_string() }.map_err(|err| err.to_string())?;
        unsafe {
            CoTaskMemFree(Some(raw.as_ptr() as _));
        }
        paths.push(PathBuf::from(path));
    }
    Ok(Some(Selection { paths }))
}

fn alloc_pwstr(text: &str) -> windows::core::Result<PWSTR> {
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();
    let ptr = unsafe { CoTaskMemAlloc(bytes) as *mut u16 };
    if ptr.is_null() {
        return Err(E_OUTOFMEMORY.into());
    }
    unsafe {
        ptr.copy_from_nonoverlapping(wide.as_ptr(), wide.len());
    }
    Ok(PWSTR::from_raw(ptr))
}

#[derive(Clone)]
enum FactoryContent {
    Root(Arc<MenuRoot>),
    Action(Arc<dyn MenuAction>),
}

#[implement(IExplorerCommand)]
struct RootImpl {
    root: Arc<MenuRoot>,
}

impl IExplorerCommand_Impl for RootImpl_Impl {
    fn GetTitle(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        alloc_pwstr(&self.root.title)
    }

    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        resolve_icon(MODULE_PATH.get().map(PathBuf::as_path), self.root.icon.as_deref())
            .and_then(|path| path.to_str().map(alloc_pwstr))
            .unwrap_or_else(|| Err(E_NOTIMPL.into()))
    }

    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        alloc_pwstr(self.root.tooltip.as_deref().unwrap_or(&self.root.title))
    }

    fn GetCanonicalName(&self) -> windows::core::Result<GUID> {
        Ok(self.root.clsid)
    }

    fn GetState(
        &self,
        _items: Option<&IShellItemArray>,
        _ok_to_be_slow: BOOL,
    ) -> windows::core::Result<u32> {
        Ok(ECS_ENABLED.0 as u32)
    }

    fn Invoke(
        &self,
        _items: Option<&IShellItemArray>,
        _bind_ctx: Option<&IBindCtx>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn GetFlags(&self) -> windows::core::Result<u32> {
        Ok(ECF_DEFAULT.0 as u32 | ECF_HASSUBCOMMANDS.0 as u32)
    }

    fn EnumSubCommands(&self) -> windows::core::Result<IEnumExplorerCommand> {
        let commands: Vec<IExplorerCommand> = (self.root.build_actions)()
            .into_iter()
            .map(|action| ActionImpl { action }.into())
            .collect();
        Ok(CommandEnum {
            commands,
            index: Cell::new(0),
        }
        .into())
    }
}

#[implement(IExplorerCommand)]
struct ActionImpl {
    action: Arc<dyn MenuAction>,
}

impl IExplorerCommand_Impl for ActionImpl_Impl {
    fn GetTitle(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        alloc_pwstr(&self.action.title())
    }

    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        resolve_icon(
            MODULE_PATH.get().map(PathBuf::as_path),
            self.action.icon().as_deref(),
        )
        .and_then(|path| path.to_str().map(alloc_pwstr))
        .unwrap_or_else(|| Err(E_NOTIMPL.into()))
    }

    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> windows::core::Result<PWSTR> {
        let tooltip = self.action.tooltip().unwrap_or_else(|| self.action.title());
        alloc_pwstr(&tooltip)
    }

    fn GetCanonicalName(&self) -> windows::core::Result<GUID> {
        self.action
            .canonical_name()
            .map(Ok)
            .unwrap_or_else(|| Err(E_NOTIMPL.into()))
    }

    fn GetState(
        &self,
        items: Option<&IShellItemArray>,
        _ok_to_be_slow: BOOL,
    ) -> windows::core::Result<u32> {
        let selection = extract_selection(items).unwrap_or(None);
        Ok(self.action.state(selection.as_ref()).to_raw())
    }

    fn Invoke(
        &self,
        items: Option<&IShellItemArray>,
        _bind_ctx: Option<&IBindCtx>,
    ) -> windows::core::Result<()> {
        let result = match extract_selection(items) {
            Ok(selection) => self.action.invoke(selection.as_ref()),
            Err(err) => Err(err),
        };
        result.map_err(|err| {
            show_error_box(&self.action.title(), &err);
            E_FAIL.into()
        })
    }

    fn GetFlags(&self) -> windows::core::Result<u32> {
        Ok(ECF_DEFAULT.0 as u32)
    }

    fn EnumSubCommands(&self) -> windows::core::Result<IEnumExplorerCommand> {
        Err(E_NOTIMPL.into())
    }
}

#[implement(IEnumExplorerCommand)]
struct CommandEnum {
    commands: Vec<IExplorerCommand>,
    index: Cell<usize>,
}

impl IEnumExplorerCommand_Impl for CommandEnum_Impl {
    fn Next(
        &self,
        celt: u32,
        puicommand: *mut Option<IExplorerCommand>,
        pceltfetched: *mut u32,
    ) -> HRESULT {
        if puicommand.is_null() {
            return E_POINTER;
        }

        let start = self.index.get();
        let remaining = self.commands.len().saturating_sub(start);
        let take = remaining.min(celt as usize);

        unsafe {
            for slot in 0..celt as usize {
                *puicommand.add(slot) = None;
            }
            for slot in 0..take {
                *puicommand.add(slot) = Some(self.commands[start + slot].clone());
            }
        }

        let fetched = take as u32;
        self.index.set(start + take);
        if !pceltfetched.is_null() {
            unsafe {
                *pceltfetched = fetched;
            }
        }

        if fetched == celt {
            HRESULT(0)
        } else {
            S_FALSE
        }
    }

    fn Skip(&self, celt: u32) -> windows::core::Result<()> {
        let next = self.index.get().saturating_add(celt as usize);
        if next <= self.commands.len() {
            self.index.set(next);
            Ok(())
        } else {
            Err(E_FAIL.into())
        }
    }

    fn Reset(&self) -> windows::core::Result<()> {
        self.index.set(0);
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IEnumExplorerCommand> {
        Ok(CommandEnum {
            commands: self.commands.clone(),
            index: Cell::new(self.index.get()),
        }
        .into())
    }
}

#[implement(IClassFactory)]
struct ClassFactory {
    content: FactoryContent,
}

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Option<&windows::core::IUnknown>,
        riid: *const GUID,
        object: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        if object.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *object = std::ptr::null_mut();
        }

        let command: IExplorerCommand = match &self.content {
            FactoryContent::Root(root) => RootImpl {
                root: Arc::clone(root),
            }
            .into(),
            FactoryContent::Action(action) => ActionImpl {
                action: Arc::clone(action),
            }
            .into(),
        };
        unsafe {
            command.query(riid, object).ok()?;
        }
        Ok(())
    }

    fn LockServer(&self, _lock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_icons_pass_through() {
        let path = Path::new(r"C:\icons\menu.ico");
        assert_eq!(
            resolve_icon(Some(Path::new(r"D:\app")), Some(path)),
            Some(path.to_path_buf())
        );
    }

    #[test]
    fn relative_icons_join_module_directory() {
        assert_eq!(
            resolve_icon(Some(Path::new(r"D:\app")), Some(Path::new("Assets\\x.ico"))),
            Some(PathBuf::from(r"D:\app\Assets\x.ico"))
        );
        assert_eq!(resolve_icon(None, Some(Path::new("Assets\\x.ico"))), None);
    }

    #[test]
    fn missing_icons_resolve_to_none() {
        assert_eq!(resolve_icon(Some(Path::new(r"D:\app")), None), None);
    }

    #[test]
    fn selection_type_helpers_report_all_files_or_dirs() {
        let empty = Selection::default();
        assert!(!empty.all_dirs());
        assert!(!empty.all_files());

        let files_only = Selection::new(vec![PathBuf::from("Cargo.toml")]);
        assert!(!files_only.all_dirs());
        assert!(files_only.all_files());
        assert_eq!(files_only.first(), Some(Path::new("Cargo.toml")));
    }

    mod com {
        use super::*;
        use windows::core::{IUnknown, Type};

        const ROOT_CLSID: GUID = GUID::from_u128(0x00000000_0000_0000_0000_000000000c01);
        const ALPHA_CLSID: GUID = GUID::from_u128(0x00000000_0000_0000_0000_00000000cafe);
        const BETA_CLSID: GUID = GUID::from_u128(0x00000000_0000_0000_0000_00000000bebe);
        const CANONICAL_CLSID: GUID = GUID::from_u128(0x00000000_0000_0000_0000_00000000ca11);
        const UNKNOWN_CLSID: GUID = GUID::from_u128(0x00000000_0000_0000_0000_00000000dead);

        struct FakeAction {
            title: &'static str,
            state: CommandState,
            outcome: Result<(), String>,
            canonical: Option<GUID>,
        }

        impl FakeAction {
            fn ok(title: &'static str) -> Arc<dyn MenuAction> {
                Arc::new(Self {
                    title,
                    state: CommandState::Enabled,
                    outcome: Ok(()),
                    canonical: None,
                })
            }

            fn failing(title: &'static str, message: &str) -> Arc<dyn MenuAction> {
                Arc::new(Self {
                    title,
                    state: CommandState::Enabled,
                    outcome: Err(message.to_string()),
                    canonical: None,
                })
            }
        }

        impl MenuAction for FakeAction {
            fn title(&self) -> String {
                self.title.to_string()
            }

            fn canonical_name(&self) -> Option<GUID> {
                self.canonical
            }

            fn state(&self, _selection: Option<&Selection>) -> CommandState {
                self.state
            }

            fn invoke(&self, _selection: Option<&Selection>) -> Result<(), String> {
                self.outcome.clone()
            }
        }

        fn sample_registration() -> Registration {
            let root = MenuRoot::new(
                ROOT_CLSID,
                "Sample",
                Box::new(|| vec![FakeAction::ok("Alpha"), FakeAction::ok("Beta")]),
            );
            let disabled = Arc::new(FakeAction {
                title: "Disabled",
                state: CommandState::Disabled,
                outcome: Ok(()),
                canonical: Some(CANONICAL_CLSID),
            });
            Registration::new(
                vec![root],
                vec![
                    StandaloneCommand::new(ALPHA_CLSID, FakeAction::ok("Alpha")),
                    StandaloneCommand::new(BETA_CLSID, disabled),
                ],
            )
        }

        fn activate(factory: &IClassFactory) -> IExplorerCommand {
            unsafe { factory.CreateInstance(None::<&IUnknown>) }.unwrap()
        }

        fn command_title(command: &IExplorerCommand) -> String {
            let raw = unsafe { command.GetTitle(None) }.unwrap();
            let text = unsafe { raw.to_string() }.unwrap();
            unsafe { CoTaskMemFree(Some(raw.as_ptr() as _)) };
            text
        }

        #[test]
        fn registration_resolves_only_known_clsids() {
            let registration = sample_registration();
            assert!(registration.factory_for(&ROOT_CLSID).is_some());
            assert!(registration.factory_for(&ALPHA_CLSID).is_some());
            assert!(registration.factory_for(&UNKNOWN_CLSID).is_none());
        }

        #[test]
        fn standalone_command_exposes_action_metadata() {
            let registration = sample_registration();
            let factory = registration.factory_for(&BETA_CLSID).unwrap();
            let command = activate(&factory);

            assert_eq!(command_title(&command), "Disabled");
            assert_eq!(
                unsafe { command.GetCanonicalName() }.unwrap(),
                CANONICAL_CLSID
            );
            assert_eq!(
                unsafe { command.GetState(None, BOOL(0)) }.unwrap(),
                ECS_DISABLED.0 as u32
            );
        }

        #[test]
        fn standalone_command_invoke_runs_the_action() {
            let registration = sample_registration();
            let factory = registration.factory_for(&ALPHA_CLSID).unwrap();
            let command = activate(&factory);

            unsafe { command.Invoke(None, None) }.unwrap();
        }

        #[test]
        fn failing_invoke_maps_to_e_fail_and_reports_through_reporter() {
            let _guard = reporter_lock();
            let registration = Registration::new(
                Vec::new(),
                vec![StandaloneCommand::new(
                    ALPHA_CLSID,
                    FakeAction::failing("Broken", "boom"),
                )],
            );
            let factory = registration.factory_for(&ALPHA_CLSID).unwrap();
            let command = activate(&factory);

            let reported = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let sink = Arc::clone(&reported);
            set_error_reporter(Some(Box::new(move |title: &str, message: &str| {
                sink.lock()
                    .unwrap()
                    .push((title.to_string(), message.to_string()));
            })));

            let result = unsafe { command.Invoke(None, None) };

            set_error_reporter(None);

            assert_eq!(result.unwrap_err().code(), E_FAIL);
            assert_eq!(
                *reported.lock().unwrap(),
                vec![("Broken".to_string(), "boom".to_string())]
            );
        }

        #[test]
        fn root_factory_enumerates_subcommands() {
            let registration = sample_registration();
            let factory = registration.factory_for(&ROOT_CLSID).unwrap();
            let command = activate(&factory);

            assert_eq!(command_title(&command), "Sample");

            let enumerator = unsafe { command.EnumSubCommands() }.unwrap();
            let mut slot: [Option<IExplorerCommand>; 1] = [None];
            let mut fetched: u32 = 0;

            assert_eq!(
                unsafe { enumerator.Next(&mut slot, Some(&mut fetched)) },
                HRESULT(0)
            );
            assert_eq!(fetched, 1);
            assert_eq!(command_title(slot[0].as_ref().unwrap()), "Alpha");

            assert_eq!(
                unsafe { enumerator.Next(&mut slot, Some(&mut fetched)) },
                HRESULT(0)
            );
            assert_eq!(fetched, 1);
            assert_eq!(command_title(slot[0].as_ref().unwrap()), "Beta");

            assert_eq!(
                unsafe { enumerator.Next(&mut slot, Some(&mut fetched)) },
                S_FALSE
            );
            assert_eq!(fetched, 0);
            assert!(slot[0].is_none());
        }

        #[test]
        fn class_factory_rejects_aggregation() {
            let registration = sample_registration();
            let factory = registration.factory_for(&ALPHA_CLSID).unwrap();

            let outer: IUnknown = factory.cast::<IUnknown>().unwrap();
            let result: windows::core::Result<IExplorerCommand> =
                unsafe { factory.CreateInstance(Some(&outer)) };

            assert_eq!(result.unwrap_err().code(), CLASS_E_NOAGGREGATION);
        }

        #[test]
        fn dll_get_class_object_serves_global_registration() {
            init(
                HINSTANCE(std::ptr::null_mut()),
                sample_registration(),
            );

            let alpha_ptr = ALPHA_CLSID;
            let riid = IClassFactory::IID;
            let mut factory_object: *mut c_void = std::ptr::null_mut();
            assert_eq!(
                unsafe { dll_get_class_object(&alpha_ptr, &riid, &mut factory_object) },
                HRESULT(0)
            );
            let factory: IClassFactory = unsafe { IClassFactory::from_abi(factory_object) }.unwrap();

            let command = activate(&factory);
            assert_eq!(command_title(&command), "Alpha");

            let mut any_object: *mut c_void = std::ptr::null_mut();
            assert_eq!(
                unsafe { dll_get_class_object(&UNKNOWN_CLSID, &riid, &mut any_object) },
                CLASS_E_CLASSNOTAVAILABLE
            );

            assert_eq!(
                unsafe {
                    dll_get_class_object(std::ptr::null(), &riid, &mut any_object)
                },
                E_POINTER
            );
        }

        #[test]
        fn error_reporter_can_be_swapped_and_restored() {
            let _guard = reporter_lock();
            let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
            let first = Arc::clone(&seen);
            set_error_reporter(Some(Box::new(move |_: &str, message: &str| {
                first.lock().unwrap().push(message.to_string());
            })));

            show_error_box("t", "first");
            set_error_reporter(None);
            set_error_reporter(Some(Box::new(|_, _| {})));
            show_error_box("t", "second");
            set_error_reporter(None);

            assert_eq!(*seen.lock().unwrap(), vec!["first".to_string()]);
        }

        static REPORTER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

        fn reporter_lock() -> std::sync::MutexGuard<'static, ()> {
            REPORTER_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }
    }
}
