import SwiftUI

struct ControlDeckStatusBar: View {
  let modules: [ControlDeckStatusModulePresentation]

  var body: some View {
    ScrollView(.horizontal, showsIndicators: false) {
      HStack(spacing: Spacing.sm) {
        ForEach(modules) { module in
          HStack(spacing: Spacing.xs) {
            Image(systemName: module.icon)
              .font(.system(size: TypeScale.micro, weight: .semibold))
            Text(module.label)
              .font(.system(size: TypeScale.caption, weight: .semibold))
              .lineLimit(1)
          }
          .foregroundStyle(module.tint)
          .padding(.horizontal, Spacing.md)
          .padding(.vertical, Spacing.sm_)
          .background(module.tint.opacity(OpacityTier.light), in: Capsule())
        }
      }
      .padding(.horizontal, Spacing.lg)
      .padding(.vertical, Spacing.md)
    }
    .scrollIndicators(.hidden)
  }
}
