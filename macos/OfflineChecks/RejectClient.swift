import Foundation

// Intentionally NOT signed as the authorized GUI. Any reply means a security failure.
@objc protocol RejectedHelperProtocol {
    func request(_ action: String, payload: Data?, reply: @escaping (Data?, String?) -> Void)
}
@main struct RejectClient {
    static func main() {
        let semaphore = DispatchSemaphore(value: 0)
        let lock = NSLock()
        var denied = false
        var finished = false
        func finish(_ rejected: Bool) {
            lock.lock(); defer { lock.unlock() }
            if !finished { finished = true; denied = rejected; semaphore.signal() }
        }
        let connection = NSXPCConnection(machServiceName: "io.github.peterrobot.sysu-netauth.gui-helper", options: .privileged)
        connection.remoteObjectInterface = NSXPCInterface(with: RejectedHelperProtocol.self)
        connection.invalidationHandler = { finish(true) }
        connection.resume()
        let proxy = connection.remoteObjectProxyWithErrorHandler { _ in finish(true) } as! RejectedHelperProtocol
        proxy.request("status", payload: nil) { _, _ in finish(false) }
        guard semaphore.wait(timeout: .now() + 10) == .success else { fputs("rejection test timed out\n", stderr); exit(1) }
        connection.invalidate()
        lock.lock(); let passed = denied; lock.unlock()
        guard passed else { fputs("SECURITY FAILURE: untrusted client received a reply\n", stderr); exit(1) }
        print("PASS: mismatched client signature rejected before dispatch")
    }
}
