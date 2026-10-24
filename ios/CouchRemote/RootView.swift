import SwiftUI

struct RootView: View {
    @AppStorage("serverURL") private var serverURL = ""
    @StateObject private var log = DiagLog()
    @State private var draft = ""
    @State private var showLog = false

    private var connected: URL? {
        guard let url = URL(string: serverURL), url.scheme == "https", url.host != nil else { return nil }
        return url
    }

    var body: some View {
        if let url = connected {
            ZStack(alignment: .topTrailing) {
                WebView(url: url, log: log)
                    .ignoresSafeArea()
                Button {
                    showLog = true
                } label: {
                    Image(systemName: "ladybug")
                        .font(.system(size: 15, weight: .semibold))
                        .padding(10)
                        .background(.ultraThinMaterial, in: Circle())
                }
                .padding(.trailing, 12)
                .padding(.top, 4)
            }
            .sheet(isPresented: $showLog) {
                DiagSheet(log: log, onChangeServer: {
                    showLog = false
                    serverURL = ""
                })
            }
        } else {
            VStack(alignment: .leading, spacing: 16) {
                Text("Connect to your PC")
                    .font(.title2.weight(.semibold))
                Text("Enter the address shown on the CouchRemote page on your PC, for example https://couch-xxxx.local:47801")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                TextField("https://…", text: $draft)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.URL)
                    .padding(12)
                    .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 10))
                Button("Connect") {
                    serverURL = draft.trimmingCharacters(in: .whitespacesAndNewlines)
                }
                .buttonStyle(.borderedProminent)
                .disabled(URL(string: draft.trimmingCharacters(in: .whitespacesAndNewlines))?.scheme != "https")
                Spacer()
            }
            .padding(20)
            .onAppear { draft = serverURL }
        }
    }
}

struct DiagSheet: View {
    @ObservedObject var log: DiagLog
    let onChangeServer: () -> Void

    var body: some View {
        NavigationStack {
            ScrollView {
                Text(log.lines.isEmpty ? "No events yet." : log.lines.joined(separator: "\n"))
                    .font(.system(size: 11, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
                    .textSelection(.enabled)
            }
            .navigationTitle("Diagnostics")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItemGroup(placement: .bottomBar) {
                    Button("Run checks") { log.runChecks() }
                    Spacer()
                    Button("Clear") { log.clear() }
                    Spacer()
                    Button("Change server", role: .destructive, action: onChangeServer)
                }
            }
        }
        .presentationDetents([.medium, .large])
    }
}
