import SwiftUI

struct ControlDeckView: View {
  let sessionId: String
  let sessionStore: SessionStore

  @State private var viewModel = ControlDeckViewModel()
  @State private var draftText = ""
  @State private var isSubmitting = false
  @FocusState private var isEditorFocused: Bool

  private var snapshot: ServerControlDeckSnapshotPayload? {
    viewModel.snapshot
  }

  private var effectivePreferences: ServerControlDeckPreferences? {
    viewModel.preferences ?? snapshot?.preferences
  }

  private var statusModules: [ControlDeckStatusModulePresentation] {
    guard let snapshot, let effectivePreferences else { return [] }
    return effectivePreferences.modules.compactMap { preference in
      guard preference.visible else { return nil }
      return ControlDeckStatusModulePresentation.make(
        module: preference.module,
        snapshot: snapshot,
        emptyVisibility: effectivePreferences.showWhenEmpty
      )
    }
  }

  private var canSubmit: Bool {
    viewModel.canSubmit && !draftText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !isSubmitting
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 0) {
      ControlDeckHeader(
        isLoading: viewModel.isLoading,
        subtitle: headerSubtitle,
        onRefresh: { Task { await viewModel.refresh() } }
      )
      Divider()
        .foregroundStyle(Color.panelBorder)
      ControlDeckEditorSection(
        draftText: $draftText,
        isSubmitting: $isSubmitting,
        isEditorFocused: $isEditorFocused,
        canSubmit: canSubmit,
        providerTitle: providerTitle,
        providerTint: providerTint,
        controlModeTitle: controlModeTitle,
        lifecycleTitle: lifecycleTitle,
        lifecycleTint: lifecycleTint,
        errorMessage: viewModel.lastError,
        onSubmit: submitDraft
      )
      if !statusModules.isEmpty {
        Divider()
          .foregroundStyle(Color.panelBorder)
        ControlDeckStatusBar(modules: statusModules)
      }
    }
    .background(Color.backgroundSecondary)
    .clipShape(RoundedRectangle(cornerRadius: Radius.xl, style: .continuous))
    .overlay(
      RoundedRectangle(cornerRadius: Radius.xl, style: .continuous)
        .strokeBorder(Color.panelBorder, lineWidth: 1)
    )
    .task(id: sessionId) {
      viewModel.bind(sessionId: sessionId, sessionStore: sessionStore)
      await viewModel.refresh()
    }
  }

  private var headerSubtitle: String {
    if viewModel.isLoading && snapshot == nil {
      return "Loading authoritative session state..."
    }
    if let snapshot {
      return "Revision \(snapshot.revision) • \(statusModules.count) active modules"
    }
    return "Server-authored input surface preview"
  }

  private var providerTitle: String {
    switch snapshot?.state.provider {
      case .claude:
        "Claude"
      case .codex:
        "Codex"
      case nil:
        "Unknown"
    }
  }

  private var providerTint: Color {
    switch snapshot?.state.provider {
      case .claude:
        Color.providerClaude
      case .codex:
        Color.providerCodex
      case nil:
        Color.textSecondary
    }
  }

  private var controlModeTitle: String {
    switch snapshot?.state.controlMode {
      case .direct:
        "Direct"
      case .passive:
        "Passive"
      case nil:
        "Unknown"
    }
  }

  private var lifecycleTitle: String {
    switch snapshot?.state.lifecycleState {
      case .open:
        "Open"
      case .resumable:
        "Resumable"
      case .ended:
        "Ended"
      case nil:
        "Unknown"
    }
  }

  private var lifecycleTint: Color {
    switch snapshot?.state.lifecycleState {
      case .open:
        Color.feedbackPositive
      case .resumable:
        Color.statusQuestion
      case .ended:
        Color.textTertiary
      case nil:
        Color.textSecondary
    }
  }

  private func submitDraft() {
    let text = draftText.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty else { return }

    isSubmitting = true
    let request = ServerControlDeckSubmitTurnRequest(
      text: text,
      attachments: [],
      skills: [],
      overrides: nil
    )

    Task {
      defer { isSubmitting = false }
      do {
        try await viewModel.submitTurn(request)
        draftText = ""
        await viewModel.refresh()
        isEditorFocused = true
      } catch {
        viewModel.lastError = String(describing: error)
      }
    }
  }
}
