use anyhow::{Result, ensure};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_long, c_void},
    sync::OnceLock,
};

const CURLOPT_URL: c_int = 10002;
const CURLOPT_USERAGENT: c_int = 10018;
const CURLOPT_WRITEDATA: c_int = 10001;
const CURLOPT_WRITEFUNCTION: c_int = 20011;
const CURLOPT_NOSIGNAL: c_int = 99;
const CURLOPT_FOLLOWLOCATION: c_int = 52;
const CURLOPT_PROTOCOLS: c_int = 181;
const CURLOPT_SSL_VERIFYPEER: c_int = 64;
const CURLOPT_SSL_VERIFYHOST: c_int = 81;
const CURLOPT_TIMEOUT_MS: c_int = 155;
const CURLOPT_CONNECTTIMEOUT_MS: c_int = 156;
const CURLINFO_RESPONSE_CODE: c_int = 0x200002;
const CURLPROTO_HTTPS: c_long = 2;

#[link(name = "curl")]
unsafe extern "C" {
    fn curl_global_init(flags: c_long) -> c_int;
    fn curl_easy_init() -> *mut c_void;
    fn curl_easy_cleanup(handle: *mut c_void);
    fn curl_easy_setopt(handle: *mut c_void, option: c_int, ...) -> c_int;
    fn curl_easy_perform(handle: *mut c_void) -> c_int;
    fn curl_easy_getinfo(handle: *mut c_void, info: c_int, ...) -> c_int;
    fn curl_easy_strerror(code: c_int) -> *const c_char;
}
struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            curl_easy_cleanup(self.0);
        }
    }
}
struct Body {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}
unsafe extern "C" fn receive(
    data: *mut c_char,
    size: usize,
    count: usize,
    context: *mut c_void,
) -> usize {
    let body = unsafe { &mut *context.cast::<Body>() };
    let Some(length) = size.checked_mul(count) else {
        return 0;
    };
    if length > body.limit.saturating_sub(body.bytes.len()) {
        body.exceeded = true;
        return 0;
    }
    if length != 0 {
        body.bytes
            .extend_from_slice(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) });
    }
    length
}
fn check(code: c_int) -> Result<()> {
    ensure!(
        code == 0,
        "HTTPS: {}",
        unsafe { CStr::from_ptr(curl_easy_strerror(code)) }.to_string_lossy()
    );
    Ok(())
}
pub fn https_get(url: &str, hosts: &[&str], limit: usize) -> Result<Vec<u8>> {
    ensure!(
        crate::update::safe_url(url, hosts),
        "untrusted download URL"
    );
    static INITIALIZED: OnceLock<c_int> = OnceLock::new();
    check(*INITIALIZED.get_or_init(|| unsafe { curl_global_init(3) }))?;
    let handle = Handle(unsafe { curl_easy_init() });
    ensure!(!handle.0.is_null(), "initialize HTTPS client");
    let url = CString::new(url)?;
    let agent = CString::new(concat!("SYSUNetAuth/", env!("NETAUTH_APP_VERSION")))?;
    let mut body = Body {
        bytes: Vec::new(),
        limit,
        exceeded: false,
    };
    // System libcurl provides certificate validation through the macOS TLS stack.
    unsafe {
        check(curl_easy_setopt(handle.0, CURLOPT_URL, url.as_ptr()))?;
        check(curl_easy_setopt(
            handle.0,
            CURLOPT_USERAGENT,
            agent.as_ptr(),
        ))?;
        check(curl_easy_setopt(
            handle.0,
            CURLOPT_WRITEDATA,
            (&mut body as *mut Body).cast::<c_void>(),
        ))?;
        check(curl_easy_setopt(
            handle.0,
            CURLOPT_WRITEFUNCTION,
            receive as unsafe extern "C" fn(*mut c_char, usize, usize, *mut c_void) -> usize,
        ))?;
        for (option, value) in [
            (CURLOPT_NOSIGNAL, 1),
            (CURLOPT_FOLLOWLOCATION, 0),
            (CURLOPT_PROTOCOLS, CURLPROTO_HTTPS),
            (CURLOPT_SSL_VERIFYPEER, 1),
            (CURLOPT_SSL_VERIFYHOST, 2),
            (CURLOPT_TIMEOUT_MS, 8000),
            (CURLOPT_CONNECTTIMEOUT_MS, 8000),
        ] {
            check(curl_easy_setopt(handle.0, option, value as c_long))?;
        }
        let result = curl_easy_perform(handle.0);
        ensure!(!body.exceeded, "download exceeds size limit");
        check(result)?;
        let mut status: c_long = 0;
        check(curl_easy_getinfo(
            handle.0,
            CURLINFO_RESPONSE_CODE,
            &mut status as *mut c_long,
        ))?;
        ensure!(status == 200, "HTTP {status}");
    }
    Ok(std::mem::take(&mut body.bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_callback_aborts_before_exceeding_download_limit() {
        let mut body = Body {
            bytes: Vec::new(),
            limit: 5,
            exceeded: false,
        };
        let mut bytes = *b"abc";
        let context = (&mut body as *mut Body).cast();
        assert_eq!(
            unsafe { receive(bytes.as_mut_ptr().cast(), 1, 3, context) },
            3
        );
        assert_eq!(
            unsafe { receive(bytes.as_mut_ptr().cast(), 1, 3, context) },
            0
        );
        assert_eq!(body.bytes, b"abc");
        assert!(body.exceeded);
    }
}
