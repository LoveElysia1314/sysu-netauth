/* Check the Rust FFI assumptions against the SDK used by the macOS runner. */
#include <stddef.h>
#include <sys/ioctl.h>
#include <net/bpf.h>
#include <net/if.h>
#include <net/if_dl.h>
#include <IOKit/IOMessage.h>
#include <CoreFoundation/CoreFoundation.h>
#include <SystemConfiguration/SystemConfiguration.h>
#include <curl/curl.h>
_Static_assert(sizeof(struct bpf_program) == 16, "BPF program layout changed");
_Static_assert(sizeof(struct ifreq) == 32, "ifreq layout changed");
_Static_assert(offsetof(struct sockaddr_dl, sdl_data) == 8, "link address layout changed");
_Static_assert(BIOCSETF == 0x80104267, "BPF filter ioctl changed");
_Static_assert(BPF_ALIGNMENT == 4, "BPF record alignment changed");
_Static_assert(offsetof(struct bpf_hdr, bh_caplen) == 8, "BPF timestamp layout changed");
_Static_assert(offsetof(struct bpf_hdr, bh_datalen) == 12, "BPF data length moved");
_Static_assert(offsetof(struct bpf_hdr, bh_hdrlen) == 16, "BPF header length moved");
_Static_assert(sizeof(CFFileDescriptorContext) == 40, "CF descriptor context changed");
_Static_assert(sizeof(SCDynamicStoreContext) == 40, "SC context changed");
_Static_assert(CURLOPT_URL == 10002 && CURLOPT_USERAGENT == 10018, "curl string options changed");
_Static_assert(CURLOPT_WRITEDATA == 10001 && CURLOPT_WRITEFUNCTION == 20011, "curl callbacks changed");
_Static_assert(CURLOPT_NOSIGNAL == 99 && CURLOPT_FOLLOWLOCATION == 52, "curl control options changed");
_Static_assert(CURLOPT_PROTOCOLS == 181 && CURL_GLOBAL_DEFAULT == 3, "curl initialization changed");
_Static_assert(CURLOPT_SSL_VERIFYPEER == 64 && CURLOPT_SSL_VERIFYHOST == 81, "curl TLS options changed");
_Static_assert(CURLOPT_TIMEOUT_MS == 155 && CURLOPT_CONNECTTIMEOUT_MS == 156, "curl timeouts changed");
_Static_assert(CURLINFO_RESPONSE_CODE == 0x200002 && CURLPROTO_HTTPS == 2, "curl protocol values changed");
_Static_assert(kIOMessageCanSystemSleep == 0xe0000270 && kIOMessageSystemWillSleep == 0xe0000280 &&
               kIOMessageSystemHasPoweredOn == 0xe0000300, "power notification values changed");
int main(void) { return 0; }
