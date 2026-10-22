/* A real SCM service, plus a foreground process, for installer lifecycle tests. */
#ifndef UNICODE
#define UNICODE
#endif
#ifndef _UNICODE
#define _UNICODE
#endif
#include <windows.h>
#include <wchar.h>

static SERVICE_STATUS_HANDLE status_handle;
static SERVICE_STATUS status;
static HANDLE stopped;
static const wchar_t *service_name;
static BOOL hang_on_stop;

static void WINAPI control(DWORD code) {
    if (code == SERVICE_CONTROL_STOP) {
        if (hang_on_stop) return; /* Simulate a service ignoring stop requests. */
        status.dwCurrentState = SERVICE_STOP_PENDING;
        status.dwControlsAccepted = 0;
        status.dwWaitHint = 1000;
        SetServiceStatus(status_handle, &status);
        SetEvent(stopped);
    }
}

static void WINAPI service_main(DWORD argc, LPWSTR *argv) {
    (void)argc;
    (void)argv;
    stopped = CreateEventW(NULL, TRUE, FALSE, NULL);
    status_handle = RegisterServiceCtrlHandlerW(service_name, control);
    if (!status_handle || !stopped) return;
    status.dwServiceType = SERVICE_WIN32_OWN_PROCESS;
    status.dwCurrentState = SERVICE_RUNNING;
    status.dwControlsAccepted = SERVICE_ACCEPT_STOP;
    SetServiceStatus(status_handle, &status);
    WaitForSingleObject(stopped, INFINITE);
    status.dwCurrentState = SERVICE_STOPPED;
    status.dwControlsAccepted = 0;
    SetServiceStatus(status_handle, &status);
    CloseHandle(stopped);
}

int wmain(int argc, wchar_t **argv) {
    if (argc >= 3 && wcscmp(argv[1], L"--service") == 0) {
        SERVICE_TABLE_ENTRYW table[2] = {{0}};
        service_name = argv[2];
        hang_on_stop = argc >= 4 && wcscmp(argv[3], L"--hang") == 0;
        table[0].lpServiceName = argv[2];
        table[0].lpServiceProc = service_main;
        return StartServiceCtrlDispatcherW(table) ? 0 : 1;
    }
    Sleep(INFINITE);
    return 0;
}
