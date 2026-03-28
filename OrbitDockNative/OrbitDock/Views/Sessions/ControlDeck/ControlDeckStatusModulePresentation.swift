import SwiftUI

struct ControlDeckStatusModulePresentation: Identifiable {
  let id: ServerControlDeckModule
  let label: String
  let icon: String
  let tint: Color

  static func make(
    module: ServerControlDeckModule,
    snapshot: ServerControlDeckSnapshotPayload,
    emptyVisibility: ServerControlDeckEmptyVisibility
  ) -> ControlDeckStatusModulePresentation? {
    let state = snapshot.state
    let config = state.config

    let resolved: (label: String?, icon: String, tint: Color) = switch module {
      case .connection:
        ("\(state.provider.rawValue.capitalized) \(state.controlMode.rawValue.capitalized)", "antenna.radiowaves.left.and.right", .accent)
      case .autonomy:
        (config.permissionMode?.replacingOccurrences(of: "_", with: " ").capitalized, "bolt.shield", .feedbackPositive)
      case .approvalMode:
        (config.permissionMode?.replacingOccurrences(of: "_", with: " ").capitalized, "lock.shield", .statusQuestion)
      case .collaborationMode:
        (config.collaborationMode?.replacingOccurrences(of: "_", with: " ").capitalized, "person.2.fill", .accent)
      case .autoReview:
        (nil, "checkmark.magnifyingglass", .feedbackPositive)
      case .tokens:
        (nil, "speedometer", .textSecondary)
      case .model:
        (config.model, "cpu", .providerCodex)
      case .effort:
        (config.effort?.capitalized, "dial.medium", .statusQuestion)
      case .branch:
        (state.gitBranch, "arrow.triangle.branch", .gitBranch)
      case .cwd:
        (preferredFolderName(currentCwd: state.currentCwd, projectPath: state.projectPath), "folder", .textSecondary)
      case .attachments:
        (nil, "paperclip", .textSecondary)
    }

    if let label = resolved.label, !label.isEmpty {
      return ControlDeckStatusModulePresentation(
        id: module,
        label: label,
        icon: resolved.icon,
        tint: resolved.tint
      )
    }

    guard emptyVisibility == .always else { return nil }
    return ControlDeckStatusModulePresentation(
      id: module,
      label: module.fallbackLabel,
      icon: resolved.icon,
      tint: resolved.tint
    )
  }

  private static func preferredFolderName(currentCwd: String?, projectPath: String) -> String {
    let preferredPath = currentCwd ?? projectPath
    let folderName = URL(fileURLWithPath: preferredPath).lastPathComponent
    return folderName.isEmpty ? preferredPath : folderName
  }
}

private extension ServerControlDeckModule {
  var fallbackLabel: String {
    switch self {
      case .connection:
        "Connection"
      case .autonomy:
        "Autonomy"
      case .approvalMode:
        "Approval"
      case .collaborationMode:
        "Collaboration"
      case .autoReview:
        "Auto Review"
      case .tokens:
        "Tokens"
      case .model:
        "Model"
      case .effort:
        "Effort"
      case .branch:
        "Branch"
      case .cwd:
        "Workspace"
      case .attachments:
        "Attachments"
    }
  }
}
