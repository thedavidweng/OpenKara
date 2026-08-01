using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Windows.Automation;

namespace OpenKara.AccessibilityProbe;

class Program
{
    [DllImport("user32.dll")]
    private static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetCursorPos(int X, int Y);

    [DllImport("user32.dll")]
    private static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint dwData, UIntPtr dwExtraInfo);

    private const int INPUT_KEYBOARD = 1;
    private const int INPUT_MOUSE = 0;
    private const uint KEYEVENTF_KEYUP = 0x0002;
    private const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
    private const uint MOUSEEVENTF_LEFTUP = 0x0004;
    private const uint MOUSEEVENTF_ABSOLUTE = 0x8000;
    private const uint MOUSEEVENTF_MOVE = 0x0001;

    [StructLayout(LayoutKind.Sequential)]
    private struct INPUT
    {
        public int type;
        public InputUnion U;
    }

    [StructLayout(LayoutKind.Explicit)]
    private struct InputUnion
    {
        [FieldOffset(0)] public KEYBDINPUT ki;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct KEYBDINPUT
    {
        public ushort wVk;
        public ushort wScan;
        public uint dwFlags;
        public uint time;
        public IntPtr dwExtraInfo;
    }

    [STAThread]
    static int Main(string[] args)
    {
        int? processId = null;
        string? processName = null;
        string? outputPath = null;
        string? windowTitle = null;
        string action = "snapshot";
        string? targetName = null;
        string? controlTypeFilter = null;
        string? valueText = null;
        string? keyName = null;
        int timeoutMs = 0;

        for (int i = 0; i < args.Length; i++)
        {
            var arg = args[i];
            if (arg == "--process-id" && i + 1 < args.Length)
            {
                if (!int.TryParse(args[++i], out int id))
                {
                    Console.Error.WriteLine("Invalid process ID.");
                    return 1;
                }
                processId = id;
            }
            else if (arg == "--process-name" && i + 1 < args.Length)
            {
                processName = args[++i];
            }
            else if (arg == "--output" && i + 1 < args.Length)
            {
                outputPath = args[++i];
            }
            else if (arg == "--window-title" && i + 1 < args.Length)
            {
                windowTitle = args[++i];
            }
            else if (arg == "--timeout" && i + 1 < args.Length)
            {
                if (!int.TryParse(args[++i], out int timeout))
                {
                    Console.Error.WriteLine("Invalid timeout value.");
                    return 1;
                }
                timeoutMs = timeout;
            }
            else if (arg == "--action" && i + 1 < args.Length)
            {
                action = args[++i].Trim().ToLowerInvariant();
            }
            else if (arg == "--name" && i + 1 < args.Length)
            {
                targetName = args[++i];
            }
            else if (arg == "--control-type" && i + 1 < args.Length)
            {
                controlTypeFilter = args[++i];
            }
            else if (arg == "--value" && i + 1 < args.Length)
            {
                valueText = args[++i];
            }
            else if (arg == "--key" && i + 1 < args.Length)
            {
                keyName = args[++i];
            }
            else
            {
                Console.Error.WriteLine($"Unknown or incomplete argument: {arg}");
                Console.Error.WriteLine(
                    "Usage: OpenKara.AccessibilityProbe --process-id <id> | --process-name <name> " +
                    "[--output <path>] [--timeout <ms>] [--window-title <title>] " +
                    "[--action snapshot|set-focus|invoke|toggle|set-value|press-key|click|double-click] [--name <substring>] " +
                    "[--control-type <type>] [--value <text>] [--key <name>]");
                return 1;
            }
        }

        if (processId is null && string.IsNullOrWhiteSpace(processName) &&
            string.IsNullOrWhiteSpace(windowTitle))
        {
            Console.Error.WriteLine("Specify --process-id, --process-name, or --window-title.");
            return 1;
        }

        if (action is not ("snapshot" or "set-focus" or "invoke" or "toggle" or "set-value" or "press-key" or "click" or "double-click"))
        {
            Console.Error.WriteLine($"Unsupported action: {action}");
            return 1;
        }

        if (action is "set-focus" or "invoke" or "toggle" or "set-value" or "click" or "double-click")
        {
            if (string.IsNullOrWhiteSpace(targetName))
            {
                Console.Error.WriteLine($"Action '{action}' requires --name <substring>.");
                return 1;
            }
        }

        if (action == "set-value" && valueText is null)
        {
            Console.Error.WriteLine("Action 'set-value' requires --value <text>.");
            return 1;
        }

        if (action == "press-key" && string.IsNullOrWhiteSpace(keyName))
        {
            Console.Error.WriteLine("Action 'press-key' requires --key <name> (enter|escape|space|tab|m|q|f|...).");
            return 1;
        }

        if (processId is null && !string.IsNullOrWhiteSpace(processName))
        {
            var matched = Process.GetProcessesByName(processName!);
            if (matched.Length == 0)
            {
                Console.Error.WriteLine($"Process not found: {processName}");
                return 1;
            }
            processId = matched[0].Id;
            foreach (var process in matched)
            {
                process.Dispose();
            }
        }

        // System file pickers may live outside the app process. Allow title-only lookup.
        var root = FindWindow(processId, timeoutMs, windowTitle);
        if (root is null)
        {
            Console.Error.WriteLine("Top-level window not found.");
            return 1;
        }

        if (action is "set-focus" or "invoke" or "toggle" or "set-value" or "press-key" or "click" or "double-click")
        {
            AutomationElement? target = null;
            if (!string.IsNullOrWhiteSpace(targetName))
            {
                target = FindNamedElement(root, targetName!, controlTypeFilter);
                if (target is null)
                {
                    Console.Error.WriteLine(
                        $"No matching element for name '{targetName}'" +
                        (string.IsNullOrWhiteSpace(controlTypeFilter) ? "" : $" control-type '{controlTypeFilter}'") +
                        ".");
                    return 2;
                }
            }
            else if (action == "press-key")
            {
                // Global app shortcuts: focus the window root before injecting keys.
                target = root;
            }

            try
            {
                if (action == "set-focus")
                {
                    target!.SetFocus();
                    Console.WriteLine($"set-focus ok: {Describe(target)}");
                }
                else if (action == "invoke")
                {
                    if (!target!.TryGetCurrentPattern(InvokePattern.Pattern, out object? pattern) || pattern is null)
                    {
                        Console.Error.WriteLine($"Element does not support Invoke: {Describe(target)}");
                        return 3;
                    }
                    ((InvokePattern)pattern).Invoke();
                    Console.WriteLine($"invoke ok: {Describe(target)}");
                }
                else if (action == "toggle")
                {
                    if (!target!.TryGetCurrentPattern(TogglePattern.Pattern, out object? pattern) || pattern is null)
                    {
                        Console.Error.WriteLine($"Element does not support Toggle: {Describe(target)}");
                        return 3;
                    }
                    ((TogglePattern)pattern).Toggle();
                    Console.WriteLine($"toggle ok: {Describe(target)}");
                }
                else if (action is "click" or "double-click")
                {
                    if (!TryMouseClick(root, target!, doubleClick: action == "double-click", out string? clickError))
                    {
                        Console.Error.WriteLine(clickError ?? $"Mouse {action} failed");
                        return 3;
                    }
                    Console.WriteLine($"{action} ok: {Describe(target!)}");
                }
                else if (action == "press-key")
                {
                    if (target is not null)
                    {
                        try
                        {
                            // Bring the hosting top-level window forward so SendInput
                            // lands in the WebView, then set UIA focus on the control.
                            try
                            {
                                var hwnd = new IntPtr(root.Current.NativeWindowHandle);
                                if (hwnd != IntPtr.Zero)
                                {
                                    SetForegroundWindow(hwnd);
                                }
                            }
                            catch (ElementNotAvailableException)
                            {
                            }

                            target.SetFocus();
                            System.Threading.Thread.Sleep(80);
                        }
                        catch (Exception ex)
                        {
                            Console.Error.WriteLine($"press-key focus warning: {ex.Message}");
                        }
                    }
                    else
                    {
                        try
                        {
                            var hwnd = new IntPtr(root.Current.NativeWindowHandle);
                            if (hwnd != IntPtr.Zero)
                            {
                                SetForegroundWindow(hwnd);
                                System.Threading.Thread.Sleep(50);
                            }
                        }
                        catch (ElementNotAvailableException)
                        {
                        }
                    }

                    if (!TrySendKeyCombo(keyName!, out string? sendError))
                    {
                        Console.Error.WriteLine(sendError ?? $"Unknown key combo: {keyName}");
                        return 3;
                    }
                    Console.WriteLine(
                        $"press-key ok: key='{keyName}'" +
                        (target is null ? "" : $" on {Describe(target)}"));
                }
                else
                {
                    if (!target!.TryGetCurrentPattern(ValuePattern.Pattern, out object? pattern) || pattern is null)
                    {
                        Console.Error.WriteLine($"Element does not support Value: {Describe(target)}");
                        return 3;
                    }
                    ((ValuePattern)pattern).SetValue(valueText!);
                    Console.WriteLine($"set-value ok: {Describe(target)} value='{valueText}'");
                }
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine($"Action '{action}' failed: {ex.Message}");
                return 4;
            }

            // Optional snapshot after the action for diagnostics.
            if (string.IsNullOrWhiteSpace(outputPath))
            {
                return 0;
            }
        }

        var nodes = new List<Node>();
        Walk(root, string.Empty, string.Empty, 0, nodes);

        var sorted = nodes
            .OrderBy(n => n.Path, StringComparer.Ordinal)
            .ToList();

        string json = JsonSerializer.Serialize(sorted, new JsonSerializerOptions
        {
            WriteIndented = true,
            Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
            PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        });

        if (!string.IsNullOrWhiteSpace(outputPath))
        {
            var directory = Path.GetDirectoryName(Path.GetFullPath(outputPath));
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory);
            }
            File.WriteAllText(outputPath, json);
        }
        else if (action == "snapshot")
        {
            Console.WriteLine(json);
        }

        return 0;
    }

    private static string Describe(AutomationElement element)
    {
        var current = TryGetCurrent(element);
        return $"{GetControlTypeName(current.ControlType)} name='{current.Name}'";
    }

    private static AutomationElement? FindNamedElement(
        AutomationElement root,
        string nameSubstring,
        string? controlTypeFilter)
    {
        var matches = new List<(AutomationElement Element, int Score)>();
        CollectNamedMatches(root, nameSubstring, controlTypeFilter, matches);
        if (matches.Count == 0)
        {
            return null;
        }

        // Prefer exact (case-insensitive) name, then shortest name, then first.
        return matches
            .OrderBy(m => m.Score)
            .ThenBy(m =>
            {
                try { return (m.Element.Current.Name ?? string.Empty).Length; }
                catch { return int.MaxValue; }
            })
            .Select(m => m.Element)
            .First();
    }

    private static void CollectNamedMatches(
        AutomationElement? element,
        string nameSubstring,
        string? controlTypeFilter,
        List<(AutomationElement Element, int Score)> matches)
    {
        if (element is null || IsStale(element))
        {
            return;
        }

        var current = TryGetCurrent(element);
        string name = current.Name ?? string.Empty;
        string controlType = GetControlTypeName(current.ControlType);

        if (!string.IsNullOrWhiteSpace(name) &&
            name.IndexOf(nameSubstring, StringComparison.OrdinalIgnoreCase) >= 0 &&
            (string.IsNullOrWhiteSpace(controlTypeFilter) ||
             controlType.Equals(controlTypeFilter, StringComparison.OrdinalIgnoreCase)) &&
            current.IsOffscreen == false)
        {
            int score = name.Equals(nameSubstring, StringComparison.OrdinalIgnoreCase) ? 0 : 1;
            // Prefer keyboard-focusable interactive controls when set-focusing.
            if (!current.IsKeyboardFocusable)
            {
                score += 10;
            }
            matches.Add((element, score));
        }

        AutomationElementCollection? children = null;
        try
        {
            children = element.FindAll(TreeScope.Children, Condition.TrueCondition);
        }
        catch (ElementNotAvailableException)
        {
        }

        if (children is null)
        {
            return;
        }

        for (int i = 0; i < children.Count; i++)
        {
            CollectNamedMatches(children[i], nameSubstring, controlTypeFilter, matches);
        }
    }

    private static AutomationElement? FindWindow(int? processId, int timeoutMs, string? windowTitle = null)
    {
        var deadline = DateTime.UtcNow.AddMilliseconds(Math.Max(timeoutMs, 0));
        do
        {
            try
            {
                var window = FindWindowOnce(processId, windowTitle);
                if (window is not null)
                {
                    return window;
                }
            }
            catch (ElementNotAvailableException)
            {
            }

            if (timeoutMs > 0)
            {
                System.Threading.Thread.Sleep(100);
            }
        } while (DateTime.UtcNow < deadline);

        return null;
    }

    private static AutomationElement? FindWindowOnce(int? processId, string? windowTitle)
    {
        // Prefer exact process match when provided.
        if (processId is int pid && pid > 0)
        {
            Condition condition;
            if (string.IsNullOrWhiteSpace(windowTitle))
            {
                condition = new AndCondition(
                    new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Window),
                    new PropertyCondition(AutomationElement.ProcessIdProperty, pid));
            }
            else
            {
                condition = new AndCondition(
                    new AndCondition(
                        new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Window),
                        new PropertyCondition(AutomationElement.ProcessIdProperty, pid)),
                    new PropertyCondition(AutomationElement.NameProperty, windowTitle));
            }

            var match = AutomationElement.RootElement.FindFirst(TreeScope.Children, condition);
            if (match is not null)
            {
                return match;
            }

            // Title may be localized or partial; scan children by substring.
            if (!string.IsNullOrWhiteSpace(windowTitle))
            {
                match = FindWindowByTitleSubstring(pid, windowTitle);
                if (match is not null)
                {
                    return match;
                }
            }
        }

        // Cross-process title lookup (Windows common file dialog host).
        if (!string.IsNullOrWhiteSpace(windowTitle))
        {
            return FindWindowByTitleSubstring(null, windowTitle);
        }

        return null;
    }

    private static AutomationElement? FindWindowByTitleSubstring(int? processId, string windowTitle)
    {
        var windows = AutomationElement.RootElement.FindAll(
            TreeScope.Children,
            new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Window));

        for (int i = 0; i < windows.Count; i++)
        {
            var window = windows[i];
            if (window is null || IsStale(window))
            {
                continue;
            }

            try
            {
                var current = window.Current;
                if (processId is int pid && pid > 0 && current.ProcessId != pid)
                {
                    continue;
                }

                string name = current.Name ?? string.Empty;
                if (name.Equals(windowTitle, StringComparison.OrdinalIgnoreCase) ||
                    name.IndexOf(windowTitle, StringComparison.OrdinalIgnoreCase) >= 0)
                {
                    return window;
                }
            }
            catch (ElementNotAvailableException)
            {
            }
        }

        return null;
    }

    private static AutomationElement.AutomationElementInformation TryGetCurrent(AutomationElement element)
    {
        try
        {
            return element.Current;
        }
        catch (ElementNotAvailableException)
        {
            return default;
        }
    }

    private static bool IsStale(AutomationElement? element)
    {
        if (element is null) return true;
        try
        {
            _ = element.Current.ControlType;
            return false;
        }
        catch (ElementNotAvailableException)
        {
            return true;
        }
    }

    private static void Walk(AutomationElement? element, string parentPath, string parent, int index, List<Node> nodes)
    {
        if (element is null || IsStale(element))
        {
            return;
        }

        var current = TryGetCurrent(element);
        string controlType = GetControlTypeName(current.ControlType);
        string name = current.Name ?? string.Empty;
        string automationId = current.AutomationId ?? string.Empty;

        string segment = $"{controlType}[{index}]";
        string path = parentPath.Length == 0 ? $"/{segment}" : $"{parentPath}/{segment}";

        var node = new Node
        {
            Path = path,
            ControlType = controlType,
            Name = name,
            AutomationId = automationId,
            IsEnabled = current.IsEnabled,
            IsFocusable = current.IsKeyboardFocusable,
            HasKeyboardFocus = current.HasKeyboardFocus,
            IsOffscreen = current.IsOffscreen,
            BoundingRectangle = GetBoundingRectangle(current),
            SupportedPatterns = GetPatternNames(element),
            IsSelected = TryGetSelected(element),
            ExpandCollapseState = TryGetExpandState(element),
            RangeValue = TryGetRangeValue(element),
            Value = TryGetValue(element),
            ToggleState = TryGetToggle(element),
            Parent = parent.Length == 0 ? null : parent,
        };

        nodes.Add(node);

        AutomationElementCollection? children = null;
        try
        {
            children = element.FindAll(TreeScope.Children, Condition.TrueCondition);
        }
        catch (ElementNotAvailableException)
        {
        }

        if (children is not null)
        {
            for (int i = 0; i < children.Count; i++)
            {
                var child = children[i];
                if (child is null || IsStale(child)) continue;

                Walk(child, path, path, i, nodes);

                var childCurrent = TryGetCurrent(child);
                string childType = GetControlTypeName(childCurrent.ControlType);
                node.Children.Add($"{path}/{childType}[{i}]");
            }
        }

        node.Children.Sort(StringComparer.Ordinal);
    }

    private static string GetControlTypeName(ControlType? controlType)
    {
        if (controlType is null)
        {
            return "Unknown";
        }
        string name = controlType.ProgrammaticName;
        const string prefix = "ControlType.";
        return name.StartsWith(prefix, StringComparison.Ordinal)
            ? name.Substring(prefix.Length)
            : name;
    }

    private static List<string> GetPatternNames(AutomationElement element)
    {
        var names = new List<string>();
        try
        {
            foreach (var pattern in element.GetSupportedPatterns())
            {
                string? name = Automation.PatternName(pattern);
                names.Add(name ?? pattern.ProgrammaticName ?? "Unknown");
            }
            names.Sort(StringComparer.Ordinal);
        }
        catch (ElementNotAvailableException)
        {
        }
        return names;
    }

    private static string? GetBoundingRectangle(AutomationElement.AutomationElementInformation current)
    {
        try
        {
            if (current.BoundingRectangle.IsEmpty)
            {
                return null;
            }
            var rect = current.BoundingRectangle;
            return FormattableString.Invariant($"{rect.Left:F1},{rect.Top:F1},{rect.Width:F1},{rect.Height:F1}");
        }
        catch (ElementNotAvailableException)
        {
            return null;
        }
    }

    private static bool? TryGetSelected(AutomationElement element)
    {
        try
        {
            if (element.TryGetCurrentPattern(SelectionItemPattern.Pattern, out object? pattern) && pattern is not null)
            {
                return ((SelectionItemPattern)pattern).Current.IsSelected;
            }
        }
        catch (ElementNotAvailableException)
        {
        }
        return null;
    }

    private static string? TryGetExpandState(AutomationElement element)
    {
        try
        {
            if (element.TryGetCurrentPattern(ExpandCollapsePattern.Pattern, out object? pattern) && pattern is not null)
            {
                return ((ExpandCollapsePattern)pattern).Current.ExpandCollapseState.ToString();
            }
        }
        catch (ElementNotAvailableException)
        {
        }
        return null;
    }

    private static double? TryGetRangeValue(AutomationElement element)
    {
        try
        {
            if (element.TryGetCurrentPattern(RangeValuePattern.Pattern, out object? pattern) && pattern is not null)
            {
                return ((RangeValuePattern)pattern).Current.Value;
            }
        }
        catch (ElementNotAvailableException)
        {
        }
        return null;
    }

    private static string? TryGetValue(AutomationElement element)
    {
        try
        {
            if (element.TryGetCurrentPattern(ValuePattern.Pattern, out object? pattern) && pattern is not null)
            {
                return ((ValuePattern)pattern).Current.Value;
            }
        }
        catch (ElementNotAvailableException)
        {
        }
        return null;
    }

    private static string? TryGetToggle(AutomationElement element)
    {
        try
        {
            if (element.TryGetCurrentPattern(TogglePattern.Pattern, out object? pattern) && pattern is not null)
            {
                return ((TogglePattern)pattern).Current.ToggleState.ToString();
            }
        }
        catch (ElementNotAvailableException)
        {
        }
        return null;
    }

    private static bool TryMouseClick(
        AutomationElement root,
        AutomationElement target,
        bool doubleClick,
        out string? error)
    {
        error = null;
        try
        {
            try
            {
                var hwnd = new IntPtr(root.Current.NativeWindowHandle);
                if (hwnd != IntPtr.Zero)
                {
                    SetForegroundWindow(hwnd);
                }
            }
            catch (ElementNotAvailableException)
            {
            }

            int x;
            int y;
            try
            {
                var point = target.GetClickablePoint();
                x = (int)Math.Round(point.X);
                y = (int)Math.Round(point.Y);
            }
            catch (NoClickablePointException)
            {
                var rect = target.Current.BoundingRectangle;
                if (rect.IsEmpty || rect.Width <= 0 || rect.Height <= 0)
                {
                    error = $"No clickable point for {Describe(target)}";
                    return false;
                }
                x = (int)Math.Round(rect.Left + rect.Width / 2.0);
                y = (int)Math.Round(rect.Top + rect.Height / 2.0);
            }

            if (!SetCursorPos(x, y))
            {
                error = $"SetCursorPos({x},{y}) failed";
                return false;
            }

            System.Threading.Thread.Sleep(40);
            mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, UIntPtr.Zero);
            mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, UIntPtr.Zero);
            if (doubleClick)
            {
                System.Threading.Thread.Sleep(40);
                mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, UIntPtr.Zero);
                mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, UIntPtr.Zero);
            }

            return true;
        }
        catch (Exception ex)
        {
            error = ex.Message;
            return false;
        }
    }

    private static bool TrySendKeyCombo(string keySpec, out string? error)
    {
        error = null;
        var parts = keySpec
            .Split(new[] { '+', '-' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Select(p => p.Trim().ToLowerInvariant())
            .Where(p => p.Length > 0)
            .ToArray();

        if (parts.Length == 0)
        {
            error = "Empty key combo.";
            return false;
        }

        var modifiers = new List<ushort>();
        ushort? mainVk = null;

        foreach (var part in parts)
        {
            if (part is "ctrl" or "control")
            {
                modifiers.Add(0x11); // VK_CONTROL
                continue;
            }
            if (part is "shift")
            {
                modifiers.Add(0x10); // VK_SHIFT
                continue;
            }
            if (part is "alt" or "menu")
            {
                modifiers.Add(0x12); // VK_MENU
                continue;
            }
            if (part is "win" or "meta" or "cmd")
            {
                modifiers.Add(0x5B); // VK_LWIN
                continue;
            }

            if (mainVk is not null)
            {
                error = $"Multiple non-modifier keys in combo '{keySpec}'.";
                return false;
            }

            mainVk = part switch
            {
                "enter" or "return" => (ushort)0x0D,
                "escape" or "esc" => (ushort)0x1B,
                "space" => (ushort)0x20,
                "tab" => (ushort)0x09,
                "left" => (ushort)0x25,
                "up" => (ushort)0x26,
                "right" => (ushort)0x27,
                "down" => (ushort)0x28,
                "f" => (ushort)0x46,
                "m" => (ushort)0x4D,
                "q" => (ushort)0x51,
                "s" => (ushort)0x53,
                "o" => (ushort)0x4F,
                "b" => (ushort)0x42,
                "comma" or "," => (ushort)0xBC,
                "period" or "." => (ushort)0xBE,
                _ when part.Length == 1 && char.IsLetterOrDigit(part[0]) =>
                    (ushort)char.ToUpperInvariant(part[0]),
                _ => (ushort)0,
            };

            if (mainVk == 0)
            {
                error = $"Unsupported key '{part}' in combo '{keySpec}'.";
                return false;
            }
        }

        if (mainVk is null)
        {
            error = $"No main key in combo '{keySpec}'.";
            return false;
        }

        var inputs = new List<INPUT>();
        foreach (var mod in modifiers)
        {
            inputs.Add(KeyDown(mod));
        }
        inputs.Add(KeyDown(mainVk.Value));
        inputs.Add(KeyUp(mainVk.Value));
        for (int i = modifiers.Count - 1; i >= 0; i--)
        {
            inputs.Add(KeyUp(modifiers[i]));
        }

        uint sent = SendInput((uint)inputs.Count, inputs.ToArray(), Marshal.SizeOf<INPUT>());
        if (sent != inputs.Count)
        {
            error = $"SendInput sent {sent}/{inputs.Count} events.";
            return false;
        }

        return true;
    }

    private static INPUT KeyDown(ushort vk) => new INPUT
    {
        type = INPUT_KEYBOARD,
        U = new InputUnion
        {
            ki = new KEYBDINPUT
            {
                wVk = vk,
                wScan = 0,
                dwFlags = 0,
                time = 0,
                dwExtraInfo = IntPtr.Zero,
            },
        },
    };

    private static INPUT KeyUp(ushort vk) => new INPUT
    {
        type = INPUT_KEYBOARD,
        U = new InputUnion
        {
            ki = new KEYBDINPUT
            {
                wVk = vk,
                wScan = 0,
                dwFlags = KEYEVENTF_KEYUP,
                time = 0,
                dwExtraInfo = IntPtr.Zero,
            },
        },
    };

    private sealed class Node
    {
        public string Path { get; set; } = string.Empty;
        public string ControlType { get; set; } = string.Empty;
        public string Name { get; set; } = string.Empty;
        public string AutomationId { get; set; } = string.Empty;
        public bool IsEnabled { get; set; }
        public bool IsFocusable { get; set; }
        public bool HasKeyboardFocus { get; set; }
        public bool IsOffscreen { get; set; }
        public string? BoundingRectangle { get; set; }
        public List<string> SupportedPatterns { get; set; } = new List<string>();
        public bool? IsSelected { get; set; }
        public string? ExpandCollapseState { get; set; }
        public double? RangeValue { get; set; }
        public string? Value { get; set; }
        public string? ToggleState { get; set; }
        public string? Parent { get; set; }
        public List<string> Children { get; set; } = new List<string>();
    }
}
