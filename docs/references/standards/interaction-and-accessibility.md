# Interaction and Accessibility

Use this profile for every rendered UI, input path, user flow, or platform
interaction.

## Authorities

| Authority                                                                                                                              | Use in OpenKara                                                                                 |
| -------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| [WCAG 2.2 AA](https://www.w3.org/TR/WCAG22/) and [ISO/IEC 40500:2025](https://www.iso.org/standard/91029.html)                         | Rendered WebView UI conformance target                                                          |
| [WCAG2ICT 2.2](https://www.w3.org/TR/wcag2ict-22/)                                                                                     | Desktop and non-web interpretation of WCAG 2.2                                                  |
| [WAI-ARIA Authoring Practices](https://www.w3.org/WAI/ARIA/apg/)                                                                       | Keyboard and state behavior for custom widgets                                                  |
| [ISO 9241-110:2020](https://www.iso.org/standard/75258.html)                                                                           | Interaction principles for task fit, clarity, user control, error tolerance, and consistency    |
| [ISO 9241-210:2019](https://www.iso.org/standard/77520.html)                                                                           | Human-centred design for changed user flows                                                     |
| Apple, [Microsoft](https://learn.microsoft.com/windows/apps/develop/accessibility), and [GNOME](https://developer.gnome.org/hig/) HIGs | Platform command names, shortcuts, menus, window behavior, and assistive technology conventions |

## Constraints

- Every action has a keyboard path. Every control has an accessible name.
- Prefer native HTML semantics. A custom widget follows its matching APG
  pattern for role, state, value, focus, and keyboard operation.
- Focus stays visible and unobscured. A suitable dialog traps focus, responds
  to Escape, and returns focus to its invoking control.
- Interactive targets meet WCAG 2.2 target-size requirements.
- Status, progress, errors, and results have the required semantic feedback.
- A material task-flow change reviews task fit, clarity, user control, error
  recovery, and consistency before acceptance.

## Required evidence

- `jsx-a11y` and the WCAG 2.2 A/AA axe suite for changed rendered UI.
- Keyboard browser tests for changed custom widgets or critical user flows.
- A platform and assistive-technology review when native window behavior,
  menus, drag operations, or OS integration changes.
