#import <AppKit/AppKit.h>
#import <dispatch/dispatch.h>
#import <objc/runtime.h>
#import <stdbool.h>

typedef struct {
    NSInteger macos_major_version;
    NSInteger tier_tag;
    NSInteger toolbar_height;
    NSInteger traffic_light_inset_leading;
    NSInteger sidebar_header_height;
} OKWindowShellProfile;

typedef NS_ENUM(NSInteger, OKWindowShellTier) {
    OKWindowShellTierDesktop = 0,
    OKWindowShellTierMac = 1,
};

static const CGFloat OKWindowShellTrafficLightTrailingGap = 14.0;
static const CGFloat OKWindowShellSidebarHeaderHeight = 28.0;
static const CGFloat OKWindowShellTrafficLightLeadingInset = 14.0;
static const CGFloat OKWindowShellTrafficLightTopInset = 14.0;
static const CGFloat OKWindowShellTrafficLightHorizontalGap = 6.0;

static void openkara_run_on_main_thread_sync(dispatch_block_t block) {
    if (block == nil) {
        return;
    }

    if ([NSThread isMainThread]) {
        block();
        return;
    }

    dispatch_sync(dispatch_get_main_queue(), block);
}

static NSRect openkara_union_rect(NSRect current, NSRect next, BOOL *hasRect) {
    if (!*hasRect) {
        *hasRect = YES;
        return next;
    }

    return NSUnionRect(current, next);
}

static BOOL openkara_layout_native_traffic_lights(
    NSWindow *window,
    CGFloat sidebarHeaderHeight,
    CGFloat *resolvedLeadingInsetOut,
    CGFloat *resolvedSidebarHeaderHeightOut
) {
    NSButton *buttons[] = {
        [window standardWindowButton:NSWindowCloseButton],
        [window standardWindowButton:NSWindowMiniaturizeButton],
        [window standardWindowButton:NSWindowZoomButton],
    };
    NSView *buttonContainer = nil;
    for (NSUInteger index = 0; index < sizeof(buttons) / sizeof(buttons[0]); index += 1) {
        NSButton *button = buttons[index];
        if (button == nil || button.superview == nil) {
            continue;
        }

        if (buttonContainer == nil) {
            buttonContainer = button.superview;
        }

        if (button.superview != buttonContainer) {
            return NO;
        }
    }

    if (buttonContainer == nil) {
        return NO;
    }

    CGFloat nextLeadingX = OKWindowShellTrafficLightLeadingInset;
    for (NSUInteger index = 0; index < sizeof(buttons) / sizeof(buttons[0]); index += 1) {
        NSButton *button = buttons[index];
        if (button == nil || button.superview != buttonContainer) {
            continue;
        }

        NSRect frame = button.frame;
        frame.origin.x = nextLeadingX;
        frame.origin.y = NSHeight(buttonContainer.bounds) - OKWindowShellTrafficLightTopInset - NSHeight(frame);
        [button setFrameOrigin:frame.origin];
        nextLeadingX += NSWidth(frame) + OKWindowShellTrafficLightHorizontalGap;
    }

    NSRect clusterBounds = NSZeroRect;
    BOOL hasClusterBounds = NO;
    for (NSUInteger index = 0; index < sizeof(buttons) / sizeof(buttons[0]); index += 1) {
        NSButton *button = buttons[index];
        if (button == nil || button.superview != buttonContainer) {
            continue;
        }
        clusterBounds = openkara_union_rect(clusterBounds, button.frame, &hasClusterBounds);
    }
    if (!hasClusterBounds) {
        return NO;
    }

    CGFloat resolvedSidebarHeaderHeight =
        MAX(sidebarHeaderHeight, NSHeight(clusterBounds) + OKWindowShellTrafficLightTopInset);

    if (resolvedLeadingInsetOut != NULL) {
        // RATIONALE: Keep the standard traffic lights in AppKit's titlebar control
        // subtree; measure their edge instead of reparenting or restyling.
        *resolvedLeadingInsetOut = NSMaxX(clusterBounds) + OKWindowShellTrafficLightTrailingGap;
    }
    if (resolvedSidebarHeaderHeightOut != NULL) {
        *resolvedSidebarHeaderHeightOut = resolvedSidebarHeaderHeight;
    }

    return YES;
}

static void openkara_configure_traffic_light_zoom_action(NSWindow *window) {
    NSButton *zoomButton = [window standardWindowButton:NSWindowZoomButton];
    if (zoomButton == nil) {
        return;
    }

    // RATIONALE: AppKit's true full-screen style intentionally stops drawing the
    // titlebar, which takes the standard traffic lights with it.  OpenKara's
    // primary workspace needs its top toolbar and native window controls to stay
    // continuously available, so the green control zooms to the usable screen
    // frame instead. `performZoom:` still provides AppKit's normal restore
    // behavior on the next click without reparenting or imitating traffic lights.
    [zoomButton setTarget:window];
    [zoomButton setAction:@selector(performZoom:)];
}

void ok_window_shell_detect_profile(OKWindowShellProfile *profile_out) {
    if (profile_out == NULL) {
        return;
    }

    NSOperatingSystemVersion version = [[NSProcessInfo processInfo] operatingSystemVersion];
    profile_out->macos_major_version = version.majorVersion;
    profile_out->tier_tag = OKWindowShellTierMac;
    profile_out->toolbar_height = 48;
    profile_out->traffic_light_inset_leading = 78;
    profile_out->sidebar_header_height = 28;
}

bool ok_window_shell_configure_main_window(
    void *ns_view_ptr,
    NSInteger tier_tag,
    double toolbar_height,
    double traffic_light_inset_leading,
    double sidebar_header_height,
    OKWindowShellProfile *profile_out
) {
    if (ns_view_ptr == NULL) {
        return false;
    }

    __block BOOL configured = NO;
    openkara_run_on_main_thread_sync(^{
        // RATIONALE: This block runs inside a Rust→C callback. An NSException that
        // escapes it unwinds through `extern "C"`, which Rust converts into
        // `panic_cannot_unwind` — the process aborts before any window reaches the
        // screen and the crash log keeps only "panic in a function that cannot
        // unwind", not the AppKit reason. AppKit raising on new OS releases is a
        // recurring pattern, so the chrome pass must degrade instead of abort:
        // `configured = NO` is the existing "use the default mac shell profile"
        // signal that `window_shell.rs` already recovers through.
        @try {
            NSView *view = (__bridge NSView *)ns_view_ptr;
            NSWindow *window = view.window;
            if (window == nil) {
                return;
            }

            window.titleVisibility = NSWindowTitleHidden;
            window.titlebarAppearsTransparent = YES;
            window.tabbingMode = NSWindowTabbingModeDisallowed;
            window.titlebarSeparatorStyle = NSTitlebarSeparatorStyleNone;
            // RATIONALE: With full-size content view + a WKWebView filling the client
            // area, movableByWindowBackground makes AppKit treat broad "background"
            // hits (including scrollbar gutters) as window moves. We already expose a
            // narrow drag affordance via data-tauri-drag-region in the web toolbar,
            // and the packaged app must keep `core:window:allow-start-dragging` in the
            // default capability or that affordance becomes a dead button.
            window.movableByWindowBackground = NO;

            // RATIONALE: windowBackgroundColor is near-white under the Light system
            // appearance, so it painted a white frame behind the WKWebView before
            // the document's own dark background composited — the white flash seen
            // at launch. Pin the native backing to the app shell's dark surface
            // (#121212) so nothing brighter than the UI can ever be exposed.
            window.backgroundColor = [NSColor colorWithSRGBRed:(18.0 / 255.0)
                                                        green:(18.0 / 255.0)
                                                         blue:(18.0 / 255.0)
                                                        alpha:1.0];

            NSWindowStyleMask styleMask = [window styleMask];
            if ((styleMask & NSWindowStyleMaskFullSizeContentView) == 0) {
                [window setStyleMask:(styleMask | NSWindowStyleMaskFullSizeContentView)];
            }

            [window setToolbar:nil];

            CGFloat resolvedLeadingInset = traffic_light_inset_leading;
            CGFloat resolvedSidebarHeaderHeight = sidebar_header_height;
            if (tier_tag != OKWindowShellTierMac) {
                return;
            }
            if (!openkara_layout_native_traffic_lights(
                window,
                MAX(sidebar_header_height, OKWindowShellSidebarHeaderHeight),
                &resolvedLeadingInset,
                &resolvedSidebarHeaderHeight
            )) {
                return;
            }
            openkara_configure_traffic_light_zoom_action(window);

            CGFloat resolvedToolbarHeight = toolbar_height;

            if (profile_out != NULL) {
                NSOperatingSystemVersion version =
                    [[NSProcessInfo processInfo] operatingSystemVersion];
                profile_out->macos_major_version = version.majorVersion;
                profile_out->tier_tag = tier_tag;
                profile_out->toolbar_height = (NSInteger)lround(resolvedToolbarHeight);
                profile_out->traffic_light_inset_leading = (NSInteger)lround(resolvedLeadingInset);
                profile_out->sidebar_header_height = (NSInteger)lround(resolvedSidebarHeaderHeight);
            }

            configured = YES;
        } @catch (NSException *exception) {
            // Re-assert the postcondition instead of relying on `configured = YES`
            // staying the last statement of the @try.
            configured = NO;
            NSLog(@"openkara window shell configuration failed: %@", exception);
        }
    });

    return configured;
}
