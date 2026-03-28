import SwiftUI

struct ControlDeckHeader: View {
  let isLoading: Bool
  let subtitle: String
  let onRefresh: () -> Void

  var body: some View {
    HStack(alignment: .top, spacing: Spacing.md) {
      VStack(alignment: .leading, spacing: Spacing.xxs) {
        Text("Control Deck")
          .font(.system(size: TypeScale.body, weight: .bold, design: .rounded))
          .foregroundStyle(Color.textPrimary)
        Text(subtitle)
          .font(.system(size: TypeScale.caption))
          .foregroundStyle(Color.textSecondary)
      }

      Spacer(minLength: 0)

      Button(action: onRefresh) {
        Image(systemName: "arrow.clockwise")
          .font(.system(size: TypeScale.caption, weight: .semibold))
          .foregroundStyle(Color.textSecondary)
          .frame(width: 28, height: 28)
          .background(Color.backgroundTertiary, in: RoundedRectangle(cornerRadius: Radius.md, style: .continuous))
      }
      .buttonStyle(.plain)
      .disabled(isLoading)
    }
    .padding(.horizontal, Spacing.lg)
    .padding(.vertical, Spacing.md)
  }
}
