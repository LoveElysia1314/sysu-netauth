//! Public CoreFoundation, SystemConfiguration and IOKit interfaces only.
use super::{Adapter, Cancel, Monitor, PowerNotifier, Signal};
use crate::engine::PowerEvent;
use anyhow::{Context, Result, ensure};
use std::{
    collections::HashMap,
    ffi::{CString, c_void},
    ptr,
    sync::{Arc, mpsc},
};

type Ref = *const c_void;
type Port = *mut c_void;
type StoreCallback = unsafe extern "C" fn(Ref, Ref, *mut c_void);
type PowerCallback = unsafe extern "C" fn(*mut c_void, u32, u32, *mut c_void);
const CAN_SLEEP: u32 = 0xe0000270;
const WILL_SLEEP: u32 = 0xe0000280;
const HAS_POWERED_ON: u32 = 0xe0000300;

#[repr(C)]
struct ContextInfo {
    version: isize,
    info: *mut c_void,
    retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<unsafe extern "C" fn(*const c_void)>,
    description: Option<unsafe extern "C" fn(*const c_void) -> Ref>,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: Ref);
    fn CFRetain(value: Ref) -> Ref;
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(value: Ref) -> u8;
    fn CFArrayGetTypeID() -> usize;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFStringCreateWithCString(allocator: Ref, value: *const i8, encoding: u32) -> Ref;
    fn CFStringGetLength(value: Ref) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFStringGetCString(value: Ref, buffer: *mut i8, size: isize, encoding: u32) -> u8;
    fn CFArrayGetCount(array: Ref) -> isize;
    fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
    fn CFArrayCreate(allocator: Ref, values: *const Ref, count: isize, callbacks: Ref) -> Ref;
    static kCFTypeArrayCallBacks: u8;
    fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
    fn CFEqual(a: Ref, b: Ref) -> u8;
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopRun();
    fn CFRunLoopStop(run_loop: Ref);
    fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFRunLoopSourceInvalidate(source: Ref);
    static kCFRunLoopDefaultMode: Ref;
    fn CFFileDescriptorCreate(
        allocator: Ref,
        fd: i32,
        close_on_invalidate: u8,
        callback: unsafe extern "C" fn(Ref, usize, *mut c_void),
        context: *const ContextInfo,
    ) -> Ref;
    fn CFFileDescriptorCreateRunLoopSource(allocator: Ref, fd: Ref, order: isize) -> Ref;
    fn CFFileDescriptorEnableCallBacks(fd: Ref, callbacks: usize);
    fn CFFileDescriptorInvalidate(fd: Ref);
}

#[link(name = "SystemConfiguration", kind = "framework")]
unsafe extern "C" {
    fn SCNetworkInterfaceCopyAll() -> Ref;
    fn SCNetworkInterfaceGetBSDName(interface: Ref) -> Ref;
    fn SCNetworkInterfaceGetLocalizedDisplayName(interface: Ref) -> Ref;
    fn SCNetworkInterfaceGetInterfaceType(interface: Ref) -> Ref;
    static kSCNetworkInterfaceTypeEthernet: Ref;
    fn SCDynamicStoreCreate(
        allocator: Ref,
        name: Ref,
        callback: Option<StoreCallback>,
        context: *const ContextInfo,
    ) -> Ref;
    fn SCDynamicStoreSetNotificationKeys(store: Ref, keys: Ref, patterns: Ref) -> u8;
    fn SCDynamicStoreCreateRunLoopSource(allocator: Ref, store: Ref, order: isize) -> Ref;
    fn SCDynamicStoreCopyValue(store: Ref, key: Ref) -> Ref;
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IORegisterForSystemPower(
        context: *mut c_void,
        port: *mut Port,
        callback: PowerCallback,
        notifier: *mut u32,
    ) -> u32;
    fn IODeregisterForSystemPower(notifier: *mut u32) -> i32;
    fn IOAllowPowerChange(connection: u32, notification: isize) -> i32;
    fn IOServiceClose(connection: u32) -> i32;
    fn IONotificationPortGetRunLoopSource(port: Port) -> Ref;
    fn IONotificationPortDestroy(port: Port);
}

struct Owned(Ref);
impl Owned {
    fn new(value: Ref) -> Result<Self> {
        ensure!(!value.is_null(), "macOS framework returned null");
        Ok(Self(value))
    }
    fn string(value: &str) -> Result<Self> {
        let string = CString::new(value)?;
        Self::new(unsafe { CFStringCreateWithCString(ptr::null(), string.as_ptr(), 0x08000100) })
    }
    fn array(values: &[Ref]) -> Result<Self> {
        Self::new(unsafe {
            CFArrayCreate(
                ptr::null(),
                values.as_ptr(),
                values.len() as isize,
                (&raw const kCFTypeArrayCallBacks).cast(),
            )
        })
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0);
        }
    }
}

// These helpers accept borrowed objects only from the framework functions above.
fn text(value: Ref) -> String {
    if value.is_null() || unsafe { CFGetTypeID(value) != CFStringGetTypeID() } {
        return String::new();
    }
    let size = unsafe { CFStringGetMaximumSizeForEncoding(CFStringGetLength(value), 0x08000100) };
    if !(0..=65536).contains(&size) {
        return String::new();
    }
    let mut bytes = vec![0u8; size as usize + 1];
    if unsafe {
        CFStringGetCString(
            value,
            bytes.as_mut_ptr().cast(),
            bytes.len() as isize,
            0x08000100,
        )
    } == 0
    {
        return String::new();
    }
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}
fn field(dictionary: Ref, key: &str) -> Ref {
    if dictionary.is_null() || unsafe { CFGetTypeID(dictionary) != CFDictionaryGetTypeID() } {
        return ptr::null();
    }
    let Ok(key) = Owned::string(key) else {
        return ptr::null();
    };
    unsafe { CFDictionaryGetValue(dictionary, key.0) }
}
fn array_values(array: Ref) -> Vec<Ref> {
    if array.is_null() || unsafe { CFGetTypeID(array) != CFArrayGetTypeID() } {
        return Vec::new();
    }
    (0..unsafe { CFArrayGetCount(array) })
        .map(|i| unsafe { CFArrayGetValueAtIndex(array, i) })
        .collect()
}

pub(super) fn ethernet_interfaces() -> Result<HashMap<String, String>> {
    let interfaces =
        Owned::new(unsafe { SCNetworkInterfaceCopyAll() }).context("enumerate macOS interfaces")?;
    let mut result = HashMap::new();
    for interface in array_values(interfaces.0) {
        let kind = unsafe { SCNetworkInterfaceGetInterfaceType(interface) };
        if kind.is_null() || unsafe { CFEqual(kind, kSCNetworkInterfaceTypeEthernet) } == 0 {
            continue;
        }
        let name = text(unsafe { SCNetworkInterfaceGetBSDName(interface) });
        if !name.is_empty() {
            result.insert(
                name,
                text(unsafe { SCNetworkInterfaceGetLocalizedDisplayName(interface) }),
            );
        }
    }
    Ok(result)
}

pub(super) fn network_details(adapters: &mut [Adapter]) {
    let Ok(name) = Owned::string("SYSU NetAuth interfaces") else {
        return;
    };
    let Ok(store) =
        Owned::new(unsafe { SCDynamicStoreCreate(ptr::null(), name.0, None, ptr::null()) })
    else {
        return;
    };
    let value = |key: &str| -> Option<Owned> {
        let key = Owned::string(key).ok()?;
        Owned::new(unsafe { SCDynamicStoreCopyValue(store.0, key.0) }).ok()
    };
    let ipv4 = value("State:/Network/Global/IPv4");
    let dns = value("State:/Network/Global/DNS");
    let primary = ipv4
        .as_ref()
        .map(|v| text(field(v.0, "PrimaryInterface")))
        .unwrap_or_default();
    let gateway = ipv4
        .as_ref()
        .map(|v| text(field(v.0, "Router")))
        .unwrap_or_default();
    let servers = dns
        .as_ref()
        .map(|v| {
            array_values(field(v.0, "ServerAddresses"))
                .into_iter()
                .map(text)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    for adapter in adapters {
        if let Some(link) = value(&format!("State:/Network/Interface/{}/Link", adapter.name)) {
            let active = field(link.0, "Active");
            if !active.is_null() && unsafe { CFGetTypeID(active) == CFBooleanGetTypeID() } {
                adapter.up &= unsafe { CFBooleanGetValue(active) } != 0;
            }
        }
        if adapter.name == primary {
            adapter.gateway = gateway.clone();
        }
        adapter.dns = servers.clone();
        adapter.score = if adapter.up { 30 } else { 0 } + if adapter.ethernet { 20 } else { -100 };
    }
}

struct RunLoopSource(Owned);
impl RunLoopSource {
    fn add(source: Ref) -> Result<Self> {
        let source = Owned::new(source)?;
        unsafe {
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source.0, kCFRunLoopDefaultMode);
        }
        Ok(Self(source))
    }
}
impl Drop for RunLoopSource {
    fn drop(&mut self) {
        unsafe {
            CFRunLoopSourceInvalidate(self.0.0);
        }
    }
}
struct Descriptor(Owned);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            CFFileDescriptorInvalidate(self.0.0);
        }
    }
}
struct PowerRegistration {
    connection: u32,
    notifier: u32,
    port: Port,
}
impl Drop for PowerRegistration {
    fn drop(&mut self) {
        unsafe {
            if self.notifier != 0 {
                IODeregisterForSystemPower(&mut self.notifier);
            }
            if self.connection != 0 {
                IOServiceClose(self.connection);
            }
            if !self.port.is_null() {
                IONotificationPortDestroy(self.port);
            }
        }
    }
}
struct Notifications {
    events: Arc<Signal>,
    power: Arc<PowerNotifier>,
    connection: u32,
}
unsafe extern "C" fn network_changed(_: Ref, _: Ref, context: *mut c_void) {
    unsafe { &*context.cast::<Notifications>() }.events.notify();
}
unsafe extern "C" fn power_changed(context: *mut c_void, _: u32, kind: u32, argument: *mut c_void) {
    let context = unsafe { &*context.cast::<Notifications>() };
    match kind {
        WILL_SLEEP => context.power.notify(PowerEvent::Suspend),
        HAS_POWERED_ON => context.power.notify(PowerEvent::Resume),
        _ => {}
    }
    if kind == CAN_SLEEP || kind == WILL_SLEEP {
        // Acknowledge immediately: authentication cleanup runs on the engine thread.
        unsafe {
            IOAllowPowerChange(context.connection, argument as isize);
        }
    }
}
unsafe extern "C" fn cancelled(_: Ref, _: usize, _: *mut c_void) {
    unsafe {
        CFRunLoopStop(CFRunLoopGetCurrent());
    }
}

fn run(
    events: Arc<Signal>,
    power: Arc<PowerNotifier>,
    stop: Arc<Cancel>,
    ready: &mpsc::SyncSender<Result<(), String>>,
) -> Result<()> {
    // Context outlives every registered callback and run-loop source, including on error.
    let mut notifications = Notifications {
        events,
        power,
        connection: 0,
    };
    let context = ContextInfo {
        version: 0,
        info: (&mut notifications as *mut Notifications).cast(),
        retain: None,
        release: None,
        description: None,
    };
    let name = Owned::string("SYSU NetAuth")?;
    let store = Owned::new(unsafe {
        SCDynamicStoreCreate(ptr::null(), name.0, Some(network_changed), &context)
    })?;
    let pattern = Owned::string("State:/Network/.*")?;
    let patterns = Owned::array(&[pattern.0])?;
    ensure!(
        unsafe { SCDynamicStoreSetNotificationKeys(store.0, ptr::null(), patterns.0) } != 0,
        "subscribe to network changes"
    );
    let _network_source =
        RunLoopSource::add(unsafe { SCDynamicStoreCreateRunLoopSource(ptr::null(), store.0, 0) })?;
    let mut registration = PowerRegistration {
        connection: 0,
        notifier: 0,
        port: ptr::null_mut(),
    };
    registration.connection = unsafe {
        IORegisterForSystemPower(
            context.info,
            &mut registration.port,
            power_changed,
            &mut registration.notifier,
        )
    };
    ensure!(
        registration.connection != 0 && !registration.port.is_null(),
        "register macOS sleep/wake notifications"
    );
    notifications.connection = registration.connection;
    let source = unsafe { IONotificationPortGetRunLoopSource(registration.port) };
    ensure!(!source.is_null(), "missing power notification source");
    let _power_source = RunLoopSource::add(unsafe { CFRetain(source) })?;
    let descriptor = Descriptor(Owned::new(unsafe {
        CFFileDescriptorCreate(ptr::null(), stop.raw(), 0, cancelled, ptr::null())
    })?);
    let _cancel_source = RunLoopSource::add(unsafe {
        CFFileDescriptorCreateRunLoopSource(ptr::null(), descriptor.0.0, 0)
    })?;
    unsafe {
        CFFileDescriptorEnableCallBacks(descriptor.0.0, 1);
    }
    let _ = ready.send(Ok(()));
    unsafe {
        CFRunLoopRun();
    }
    Ok(())
}

pub(super) fn monitor(events: Arc<Signal>, power: Arc<PowerNotifier>) -> Result<Monitor> {
    let stop = Arc::new(Cancel::new()?);
    let done = stop.clone();
    let (tx, rx) = mpsc::sync_channel(1);
    let thread = std::thread::Builder::new()
        .name("macos-notifications".into())
        .spawn(move || {
            if let Err(error) = run(events, power, done, &tx) {
                let _ = tx.send(Err(format!("{error:#}")));
            }
        })?;
    let monitor = Monitor {
        stop,
        thread: Some(thread),
    };
    rx.recv()
        .context("macOS notification worker exited")?
        .map_err(anyhow::Error::msg)?;
    Ok(monitor)
}
