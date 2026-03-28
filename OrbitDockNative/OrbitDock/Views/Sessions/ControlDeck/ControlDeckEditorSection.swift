import SwiftUI

struct ControlDeckEditorSection: View {
  @Binding var draftText: String
  @Binding var isSubmitting: Bool
  @FocusState.Binding var isEditorFocused: Bool
  let canSubmit: Bool
  let providerTitle: String
  let providerTint: Color
  let controlModeTitle: String
  let lifecycleTitle: String
  let lifecycleTint: Color
  let errorMessage: String?
  let onSubmit: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: Spacing.md) {
      ZStack(alignment: .topLeading) {
        RoundedRectangle(cornerRadius: Radius.lg, style: .continuous)
          .fill(Color.backgroundPrimary)

        TextEditor(text: $draftText)
          .focused($isEditorFocused)
          .scrollContentBackground(.hidden)
          .font(.system(size: TypeScale.body))
          .foregroundStyle(Color.textPrimary)
          .frame(minHeight: 108)
          .padding(.horizontal, Spacing.sm)
          .padding(.vertical, Spacing.sm_)

        if draftText.isEmpty {
          Text("Message the session through the new server-driven Control Deck surface.")
            .font(.system(size: TypeScale.body))
            .foregroundStyle(Color.textTertiary)
            .padding(.horizontal, Spacing.md)
            .padding(.vertical, Spacing.md_)
            .allowsHitTesting(false)
        }
      }
      .overlay(
        RoundedRectangle(cornerRadius: Radius.lg, style: .continuous)
          .strokeBorder(Color.panelBorder, lineWidth: 1)
      )

      if let errorMessage, !errorMessage.isEmpty {
        Text(errorMessage)
          .font(.system(size: TypeScale.caption, weight: .medium, design: .monospaced))
          .foregroundStyle(Color.statusPermission)
          .textSelection(.enabled)
      }

      HStack(spacing: Spacing.sm) {
        ControlDeckStateBadge(label: "Provider", value: providerTitle, tint: providerTint)
        ControlDeckStateBadge(label: "Mode", value: controlModeTitle, tint: Color.accent)
        ControlDeckStateBadge(label: "Lifecycle", value: lifecycleTitle, tint: lifecycleTint)

        Spacer(minLength: 0)

        Button(action: onSubmit) {
          HStack(spacing: Spacing.xs) {
            if isSubmitting {
              ProgressView()
                .controlSize(.small)
            } else {
              Image(systemName: "arrow.up.circle.fill")
                .font(.system(size: TypeScale.caption, weight: .semibold))
            }

            Text(isSubmitting ? "Sending..." : "Send")
              .font(.system(size: TypeScale.caption, weight: .semibold))
          }
          .foregroundStyle(canSubmit ? Color.backgroundPrimary : Color.textTertiary)
          .padding(.horizontal, Spacing.md)
          .padding(.vertical, Spacing.sm_)
          .background(canSubmit ? Color.accent : Color.backgroundTertiary, in: Capsule())
        }
        .buttonStyle(.plain)
        .disabled(!canSubmit)
      }
    }
    .padding(Spacing.lg)
  }
}

private struct ControlDeckStateBadge: View {
  let label: String
  let value: String
  let tint: Color

  var body: some View {
    VStack(alignment: .leading, spacing: 2) {
      Text(label.uppercased())
        .font(.system(size: TypeScale.micro, weight: .semibold))
        .foregroundStyle(Color.textTertiary)
      Text(value)
        .font(.system(size: TypeScale.caption, weight: .semibold))
        .foregroundStyle(tint)
    }
    .padding(.horizontal, Spacing.sm)
    .padding(.vertical, Spacing.xs)
    .background(Color.backgroundTertiary, in: RoundedRectangle(cornerRadius: Radius.md, style: .continuous))
  }
}
