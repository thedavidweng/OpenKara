using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Windows.Automation;

namespace OpenKara.AccessibilityProbe;

class Program
{
    [STAThread]
    static int Main(string[] args)
    {
        int? processId = null;
        string? processName = null;
        string? outputPath = null;
        string? windowTitle = null;
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
            else
            {
                Console.Error.WriteLine($"Unknown or incomplete argument: {arg}");
                Console.Error.WriteLine("Usage: OpenKara.AccessibilityProbe --process-id <id> | --process-name <name> [--output <path>] [--timeout <ms>] [--window-title <title>]");
                return 1;
            }
        }

        if (processId is null && string.IsNullOrWhiteSpace(processName))
        {
            Console.Error.WriteLine("Specify --process-id or --process-name.");
            return 1;
        }

        if (processId is null)
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

        var root = FindWindow(processId.Value, timeoutMs, windowTitle);
        if (root is null)
        {
            Console.Error.WriteLine("Top-level window not found.");
            return 1;
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
        else
        {
            Console.WriteLine(json);
        }

        return 0;
    }

    private static AutomationElement? FindWindow(int processId, int timeoutMs, string? windowTitle = null)
    {
        Condition condition;
        if (string.IsNullOrWhiteSpace(windowTitle))
        {
            condition = new AndCondition(
                new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Window),
                new PropertyCondition(AutomationElement.ProcessIdProperty, processId));
        }
        else
        {
            condition = new AndCondition(
                new AndCondition(
                    new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Window),
                    new PropertyCondition(AutomationElement.ProcessIdProperty, processId)),
                new PropertyCondition(AutomationElement.NameProperty, windowTitle));
        }

        var deadline = DateTime.UtcNow.AddMilliseconds(Math.Max(timeoutMs, 0));
        do
        {
            try
            {
                var window = AutomationElement.RootElement.FindFirst(TreeScope.Children, condition);
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
