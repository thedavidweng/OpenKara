using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Windows;
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
            else
            {
                Console.Error.WriteLine($"Unknown or incomplete argument: {arg}");
                Console.Error.WriteLine("Usage: OpenKara.AccessibilityProbe --process-id <id> | --process-name <name> [--output <path>]");
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

        var root = FindWindow(processId.Value);
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

    private static AutomationElement? FindWindow(int processId)
    {
        var condition = new AndCondition(
            new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.Window),
            new PropertyCondition(AutomationElement.ProcessIdProperty, processId));

        return AutomationElement.RootElement.FindFirst(TreeScope.Children, condition);
    }

    private static void Walk(AutomationElement element, string parentPath, string parent, int index, List<Node> nodes)
    {
        string controlType = GetControlTypeName(element.Current.ControlType);
        string name = element.Current.Name ?? string.Empty;
        string automationId = element.Current.AutomationId ?? string.Empty;

        string segment = $"{controlType}[{index}]";
        string path = parentPath.Length == 0 ? $"/{segment}" : $"{parentPath}/{segment}";

        var node = new Node
        {
            Path = path,
            ControlType = controlType,
            Name = name,
            AutomationId = automationId,
            IsEnabled = element.Current.IsEnabled,
            IsFocusable = element.Current.IsKeyboardFocusable,
            IsOffscreen = element.Current.IsOffscreen,
            BoundingRectangle = GetBoundingRectangle(element),
            SupportedPatterns = GetPatternNames(element),
            IsSelected = TryGetSelected(element),
            ExpandCollapseState = TryGetExpandState(element),
            RangeValue = TryGetRangeValue(element),
            Parent = parent.Length == 0 ? null : parent,
        };

        nodes.Add(node);

        var child = TreeWalker.RawViewWalker.GetFirstChild(element);
        int childIndex = 0;
        while (child is not null)
        {
            Walk(child, path, path, childIndex, nodes);
            node.Children.Add($"{path}/{GetControlTypeName(child.Current.ControlType)}[{childIndex}]");
            child = TreeWalker.RawViewWalker.GetNextSibling(child);
            childIndex++;
        }

        node.Children.Sort(StringComparer.Ordinal);
    }

    private static string GetControlTypeName(ControlType controlType)
    {
        string name = controlType.ProgrammaticName;
        const string prefix = "ControlType.";
        return name.StartsWith(prefix, StringComparison.Ordinal)
            ? name.Substring(prefix.Length)
            : name;
    }

    private static List<string> GetPatternNames(AutomationElement element)
    {
        var names = new List<string>();
        foreach (var pattern in element.GetSupportedPatterns())
        {
            string? name = Automation.PatternName(pattern);
            names.Add(name ?? pattern.ProgrammaticName ?? "Unknown");
        }
        names.Sort(StringComparer.Ordinal);
        return names;
    }

    private static string? GetBoundingRectangle(AutomationElement element)
    {
        Rect rect = element.Current.BoundingRectangle;
        if (rect.IsEmpty)
        {
            return null;
        }
        return FormattableString.Invariant($"{rect.Left:F1},{rect.Top:F1},{rect.Width:F1},{rect.Height:F1}");
    }

    private static bool? TryGetSelected(AutomationElement element)
    {
        if (element.TryGetCurrentPattern(SelectionItemPattern.Pattern, out object? pattern))
        {
            return ((SelectionItemPattern)pattern!).Current.IsSelected;
        }
        return null;
    }

    private static string? TryGetExpandState(AutomationElement element)
    {
        if (element.TryGetCurrentPattern(ExpandCollapsePattern.Pattern, out object? pattern))
        {
            return ((ExpandCollapsePattern)pattern!).Current.ExpandCollapseState.ToString();
        }
        return null;
    }

    private static double? TryGetRangeValue(AutomationElement element)
    {
        if (element.TryGetCurrentPattern(RangeValuePattern.Pattern, out object? pattern))
        {
            return ((RangeValuePattern)pattern!).Current.Value;
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
        public bool IsOffscreen { get; set; }
        public string? BoundingRectangle { get; set; }
        public List<string> SupportedPatterns { get; set; } = new List<string>();
        public bool? IsSelected { get; set; }
        public string? ExpandCollapseState { get; set; }
        public double? RangeValue { get; set; }
        public string? Parent { get; set; }
        public List<string> Children { get; set; } = new List<string>();
    }
}
