import Observation
import Foundation

@MainActor
@Observable
final class ControlDeckViewModel {
  var currentSessionId: String?
  var currentSessionStore = SessionStore.preview()
  var snapshot: ServerControlDeckSnapshotPayload?
  var preferences: ServerControlDeckPreferences?
  var isLoading = false
  var lastError: String?

  func bind(sessionId: String, sessionStore: SessionStore) {
    currentSessionId = sessionId
    currentSessionStore = sessionStore
  }

  private var resolvedSessionId: String {
    currentSessionId ?? ""
  }

  var canSubmit: Bool {
    snapshot?.state.acceptsUserInput == true
  }

  func refresh() async {
    guard !resolvedSessionId.isEmpty else { return }
    isLoading = true
    defer { isLoading = false }

    do {
      async let snapshotTask = currentSessionStore.fetchControlDeckSnapshot(sessionId: resolvedSessionId)
      async let preferencesTask = currentSessionStore.fetchControlDeckPreferences()
      snapshot = try await snapshotTask
      preferences = try await preferencesTask
      lastError = nil
    } catch {
      lastError = String(describing: error)
    }
  }

  func updatePreferences(_ request: ServerControlDeckPreferences) async throws {
    let updated = try await currentSessionStore.updateControlDeckPreferences(request)
    preferences = updated
  }

  func submitTurn(_ request: ServerControlDeckSubmitTurnRequest) async throws {
    try await currentSessionStore.submitControlDeckTurn(
      sessionId: resolvedSessionId,
      request: request
    )
  }
}
