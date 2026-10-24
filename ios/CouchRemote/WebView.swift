import SwiftUI
import WebKit

/// Hosts the CouchRemote web app. TLS is accepted only when the server certificate chains to the CouchRemote
/// root certificate this phone stored while pairing, and only for the paired PC's name.
struct WebView: UIViewRepresentable {
    let url: URL
    let ca: SecCertificate
    let log: DiagLog
    let onLoaded: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(host: url.host ?? "", ca: ca, log: log, onLoaded: onLoaded) }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.allowsInlineMediaPlayback = true
        let controller = WKUserContentController()
        controller.add(context.coordinator, name: "diag")
        controller.addUserScript(WKUserScript(source: Coordinator.bridgeScript, injectionTime: .atDocumentStart, forMainFrameOnly: false))
        config.userContentController = controller

        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        view.uiDelegate = context.coordinator
        view.isOpaque = false
        view.scrollView.contentInsetAdjustmentBehavior = .never
        log.webView = view
        log.add("load \(url.absoluteString)")
        view.load(URLRequest(url: url))
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) {}

    final class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate, WKScriptMessageHandler {
        let host: String
        let ca: SecCertificate
        let log: DiagLog
        let onLoaded: () -> Void
        init(host: String, ca: SecCertificate, log: DiagLog, onLoaded: @escaping () -> Void) {
            self.host = host; self.ca = ca; self.log = log; self.onLoaded = onLoaded
        }

        /// Forwards console output, script errors and WebSocket lifecycle events to the native log.
        static let bridgeScript = """
        (function () {
          function send(kind, msg) { try { window.webkit.messageHandlers.diag.postMessage(kind + ': ' + msg); } catch (e) {} }
          ['log', 'warn', 'error'].forEach(function (level) {
            var orig = console[level];
            console[level] = function () {
              send('console.' + level, Array.prototype.slice.call(arguments).join(' '));
              orig.apply(console, arguments);
            };
          });
          // The shell is the installed app: skip the "Add to Home Screen" guide the web page shows in Safari.
          try { Object.defineProperty(navigator, 'standalone', { get: function () { return true; } }); } catch (e) {}
          window.addEventListener('error', function (e) { send('error', e.message); });
          window.addEventListener('unhandledrejection', function (e) { send('rejection', String(e.reason)); });
          var Native = window.WebSocket;
          function Wrapped(url, protocols) {
            var ws = protocols === undefined ? new Native(url) : new Native(url, protocols);
            ws.addEventListener('open', function () { send('ws', 'open ' + url); });
            ws.addEventListener('close', function (e) { send('ws', 'close ' + e.code + ' ' + url); });
            ws.addEventListener('error', function () { send('ws', 'error ' + url); });
            return ws;
          }
          Wrapped.prototype = Native.prototype;
          Wrapped.CONNECTING = 0; Wrapped.OPEN = 1; Wrapped.CLOSING = 2; Wrapped.CLOSED = 3;
          window.WebSocket = Wrapped;
        })();
        """

        func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
            log.add("\(message.body)")
        }

        /// The air mouse reads the phone's motion sensors. Without this answer the page's permission request can hang;
        /// the shell grants it for the paired PC only.
        func webView(_ webView: WKWebView, requestDeviceOrientationAndMotionPermissionFor origin: WKSecurityOrigin,
                     initiatedByFrame frame: WKFrameInfo, decisionHandler: @escaping (WKPermissionDecision) -> Void) {
            let allowed = origin.host == host
            log.add("motion permission \(allowed ? "granted" : "denied") for \(origin.host)")
            decisionHandler(allowed ? .grant : .deny)
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            log.add("page loaded: \(webView.url?.absoluteString ?? "?")")
            onLoaded()
        }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            log.add("navigation failed: \(error.localizedDescription)")
        }

        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
            log.add("load failed: \(error.localizedDescription)")
        }

        func webView(_ webView: WKWebView, didReceive challenge: URLAuthenticationChallenge,
                     completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
            let space = challenge.protectionSpace
            guard space.authenticationMethod == NSURLAuthenticationMethodServerTrust,
                  let trust = space.serverTrust else {
                completionHandler(.performDefaultHandling, nil)
                return
            }
            guard space.host == host else {
                log.add("tls: refused \(space.host), not the paired PC")
                completionHandler(.cancelAuthenticationChallenge, nil)
                return
            }
            SecTrustSetAnchorCertificates(trust, [ca] as CFArray)
            SecTrustSetAnchorCertificatesOnly(trust, true)
            var error: CFError?
            if SecTrustEvaluateWithError(trust, &error) {
                log.add("tls: \(space.host):\(space.port) chains to the paired CouchRemote CA")
                completionHandler(.useCredential, URLCredential(trust: trust))
            } else {
                log.add("tls: REJECTED \(space.host): \(error?.localizedDescription ?? "unknown")")
                completionHandler(.cancelAuthenticationChallenge, nil)
            }
        }
    }
}
