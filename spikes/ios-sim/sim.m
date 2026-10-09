// Spike: show a booted iOS Simulator device's screen in our own window and
// send clicks to it as touches, using Xcode's private SimulatorKit and
// CoreSimulator frameworks (no Simulator.app).
//
//   sim --udid UDID [--tap X,Y] [--capture PATH] [--exit-after SECONDS] [--front]
//
// --tap sends a tap at X,Y (fractions of the screen, from the top left) after
// 3 s; --capture saves our window's pixels after 4 s. Without --front the
// window opens behind other apps' windows and the app never activates.

#import <AppKit/AppKit.h>
#import <dlfcn.h>
#import <objc/message.h>

static NSString *const kSimulatorKit = @"/Applications/Xcode.app/Contents/Developer/Library/PrivateFrameworks/SimulatorKit.framework/SimulatorKit";
static NSString *const kCoreSimulator = @"/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator";

// IndigoHIDMessageForMouseNSEvent(point, point2, target, eventType, size, edge),
// read from the disassembly: ints in x0–x4, the size in d0/d1.
typedef void *(*IndigoMouseFn)(CGPoint *point, CGPoint *point2, uint32_t target, NSEventType type, NSSize size, uint32_t edge);
static IndigoMouseFn IndigoMouse;
// IndigoHIDMessageForButton(button, direction, target): three ints, from the disassembly.
// Values as in Facebook's idb: Home is button 0, down 1 / up 2, buttons go to target 0x33.
typedef void *(*IndigoButtonFn)(uint32_t button, uint32_t direction, uint32_t target);
static IndigoButtonFn IndigoButton;
static uint32_t gTarget = 0x32; // the touch screen
static double gT0;
static double gTouchSent;
static long gEdgeOverride = -1; // --edge N: try another IndigoHIDEdge value // a touch waiting for the screen to change

static double now(void) { return [NSDate timeIntervalSinceReferenceDate]; }
static void say(NSString *format, ...) {
    va_list args; va_start(args, format);
    NSString *line = [[NSString alloc] initWithFormat:format arguments:args];
    va_end(args);
    fprintf(stderr, "[%7.3f] %s\n", now() - gT0, line.UTF8String);
}

static id device(NSString *udid) {
    NSError *error = nil;
    Class context = NSClassFromString(@"SimServiceContext");
    id ctx = ((id (*)(id, SEL, id, NSError **))objc_msgSend)(context, NSSelectorFromString(@"sharedServiceContextForDeveloperDir:error:"),
                                                             @"/Applications/Xcode.app/Contents/Developer", &error);
    id set = ((id (*)(id, SEL, NSError **))objc_msgSend)(ctx, NSSelectorFromString(@"defaultDeviceSetWithError:"), &error);
    for (id d in [set valueForKey:@"devices"]) {
        if ([[[d valueForKey:@"UDID"] UUIDString] isEqualToString:udid]) return d;
    }
    say(@"no device %@ (%@)", udid, error);
    return nil;
}

/// Turns clicks into touches on the device.
@interface TouchView : NSView
@property(strong) id hid;
@property NSSize screen; // device screen in points, for the message
@property uint32_t edge; // the screen edge a gesture started at (IndigoHIDEdge), for system swipes
@end

@implementation TouchView
- (BOOL)acceptsFirstMouse:(NSEvent *)event { return YES; }
- (BOOL)acceptsFirstResponder { return YES; }
- (void)deliver:(void *)message label:(NSString *)label {
    ((void (*)(id, SEL, void *, BOOL, id, id))objc_msgSend)(self.hid, NSSelectorFromString(@"sendWithMessage:freeWhenDone:completionQueue:completion:"),
        message, YES, dispatch_get_main_queue(), ^(NSError *error) { say(@"%@ %@", label, error ?: @"ok"); });
}
/// The Home button: press and release.
- (void)home {
    [self deliver:IndigoButton(0, 1, 0x33) label:@"home down"];
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 60 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{ [self deliver:IndigoButton(0, 2, 0x33) label:@"home up"]; });
}
- (BOOL)performKeyEquivalent:(NSEvent *)event {
    // ⇧⌘H, as in Simulator.app.
    if ((event.modifierFlags & NSEventModifierFlagDeviceIndependentFlagsMask) == (NSEventModifierFlagCommand | NSEventModifierFlagShift)
        && [event.charactersIgnoringModifiers.lowercaseString isEqualToString:@"h"]) { [self home]; return YES; }
    return [super performKeyEquivalent:event];
}
- (void)send:(NSEventType)type at:(NSPoint)ratio {
    CGPoint point = CGPointMake(ratio.x, ratio.y);
    if (type == NSEventTypeLeftMouseDown) {
        // IndigoHIDEdge: 2 did something for a bottom swipe (switched apps); the rest is a guess.
        const double inset = 0.03;
        self.edge = ratio.y > 1 - inset ? 2 : ratio.y < inset ? 1 : ratio.x < inset ? 3 : ratio.x > 1 - inset ? 4 : 0;
        if (gEdgeOverride >= 0 && self.edge) self.edge = (uint32_t)gEdgeOverride;
    }
    void *message = IndigoMouse(&point, NULL, gTarget, type, NSMakeSize(1, 1), self.edge);
    if (!message) { say(@"no message"); return; }
    double sent = now();
    if (type == NSEventTypeLeftMouseDown) gTouchSent = sent;
    ((void (*)(id, SEL, void *, BOOL, id, id))objc_msgSend)(self.hid, NSSelectorFromString(@"sendWithMessage:freeWhenDone:completionQueue:completion:"),
        message, YES, dispatch_get_main_queue(), ^(NSError *error) {
            if (type != NSEventTypeLeftMouseDragged) say(@"touch %lu at %.3f,%.3f edge %u %@ (%.1f ms)", (unsigned long)type, ratio.x, ratio.y, self.edge, error ?: @"ok", (now() - sent) * 1000);
        });
}
- (NSPoint)ratio:(NSEvent *)event {
    NSPoint p = [self convertPoint:event.locationInWindow fromView:nil];
    return NSMakePoint(p.x / self.bounds.size.width, 1 - p.y / self.bounds.size.height);
}
- (void)mouseDown:(NSEvent *)event { [self send:NSEventTypeLeftMouseDown at:[self ratio:event]]; }
- (void)mouseDragged:(NSEvent *)event { [self send:NSEventTypeLeftMouseDragged at:[self ratio:event]]; }
- (void)mouseUp:(NSEvent *)event { [self send:NSEventTypeLeftMouseUp at:[self ratio:event]]; }
@end

static void capture(NSWindow *window, NSString *path) {
    typedef CGImageRef (*CreateImage)(CGRect, uint32_t, uint32_t, uint32_t);
    CreateImage create = (CreateImage)dlsym(RTLD_DEFAULT, "CGWindowListCreateImage");
    CGImageRef image = create(CGRectNull, 1 << 3 /* including window */, (uint32_t)window.windowNumber, 1 | 8);
    if (!image) { say(@"capture failed"); return; }
    NSBitmapImageRep *rep = [[NSBitmapImageRep alloc] initWithCGImage:image];
    [[rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}] writeToFile:path atomically:YES];
    CGImageRelease(image);
    say(@"captured %@ (%zux%zu)", path, rep.pixelsWide, rep.pixelsHigh);
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        gT0 = now();
        NSUserDefaults *args = [NSUserDefaults standardUserDefaults]; // -udid X style also works
        NSMutableDictionary *opts = [NSMutableDictionary dictionary];
        for (int i = 1; i < argc; i++) {
            NSString *a = @(argv[i]);
            if (![a hasPrefix:@"--"]) continue;
            NSString *key = [a substringFromIndex:2];
            if (i + 1 < argc && ![@(argv[i + 1]) hasPrefix:@"--"]) opts[key] = @(argv[++i]); else opts[key] = @YES;
        }
        (void)args;
        if (!dlopen(kCoreSimulator.UTF8String, RTLD_NOW) || !dlopen(kSimulatorKit.UTF8String, RTLD_NOW)) { say(@"dlopen: %s", dlerror()); return 1; }
        IndigoMouse = (IndigoMouseFn)dlsym(RTLD_DEFAULT, "IndigoHIDMessageForMouseNSEvent");
        IndigoButton = (IndigoButtonFn)dlsym(RTLD_DEFAULT, "IndigoHIDMessageForButton");
        if (opts[@"edge"]) gEdgeOverride = [opts[@"edge"] integerValue];
        if (opts[@"target"]) gTarget = (uint32_t)strtoul([opts[@"target"] UTF8String], NULL, 0);

        id dev = device(opts[@"udid"]);
        if (!dev) return 1;
        say(@"device %@ state %@", [dev valueForKey:@"name"], [dev valueForKey:@"stateString"]);

        NSApplication *app = [NSApplication sharedApplication];
        BOOL front = opts[@"front"] != nil;
        app.activationPolicy = front ? NSApplicationActivationPolicyRegular : NSApplicationActivationPolicyAccessory;

        NSWindow *window = [[NSWindow alloc] initWithContentRect:NSMakeRect(200, 200, 402, 874)
                                                       styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskResizable
                                                         backing:NSBackingStoreBuffered defer:NO];
        window.title = [NSString stringWithFormat:@"spike: %@", [dev valueForKey:@"name"]];
        NSView *content = window.contentView;
        content.wantsLayer = YES;

        // The device's main display, as an IOSurface we put in a layer.
        NSView *display = [[NSView alloc] initWithFrame:content.bounds];
        display.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
        display.wantsLayer = YES;
        display.layer.contentsGravity = kCAGravityResizeAspect;
        display.layer.backgroundColor = NSColor.blackColor.CGColor;
        [content addSubview:display];
        id io = ((id (*)(id, SEL))objc_msgSend)(dev, NSSelectorFromString(@"io"));
        id screen = nil;
        for (id port in ((id (*)(id, SEL))objc_msgSend)(io, NSSelectorFromString(@"ioPorts"))) {
            id desc = ((id (*)(id, SEL))objc_msgSend)(port, NSSelectorFromString(@"descriptor"));
            if (![desc respondsToSelector:NSSelectorFromString(@"framebufferSurface")]) continue;
            id state = ((id (*)(id, SEL))objc_msgSend)(desc, NSSelectorFromString(@"state"));
            unsigned short displayClass = ((unsigned short (*)(id, SEL))objc_msgSend)(state, NSSelectorFromString(@"displayClass"));
            say(@"display port %@ class %u", desc, displayClass);
            if (displayClass == 0 && !screen) screen = desc;
        }
        if (!screen) { say(@"no main display port"); return 1; }
        __block NSUInteger frames = 0;
        CALayer *layer = display.layer;
        void (^show)(void) = ^{
            id surface = ((id (*)(id, SEL))objc_msgSend)(screen, NSSelectorFromString(@"framebufferSurface"));
            layer.contents = surface;
            say(@"framebuffer %@", surface);
        };
        show();
        NSUUID *uuid = [NSUUID UUID];
        ((void (*)(id, SEL, id, id))objc_msgSend)(screen, NSSelectorFromString(@"registerCallbackWithUUID:ioSurfacesChangeCallback:"), uuid,
            ^(id a, id b) { dispatch_async(dispatch_get_main_queue(), ^{ say(@"surfaces changed"); show(); }); });
        ((void (*)(id, SEL, id, id))objc_msgSend)(screen, NSSelectorFromString(@"registerCallbackWithUUID:damageRectanglesCallback:"), uuid,
            ^(NSArray *rects) { double at = now(); dispatch_async(dispatch_get_main_queue(), ^{ frames++;
                if (gTouchSent) { say(@"touch → first screen update: %.1f ms", (at - gTouchSent) * 1000); gTouchSent = 0; } ((void (*)(id, SEL))objc_msgSend)(layer, NSSelectorFromString(@"setContentsChanged")); }); });
        [NSTimer scheduledTimerWithTimeInterval:1 repeats:YES block:^(NSTimer *t) { if (frames) say(@"%lu frames/s", (unsigned long)frames); frames = 0; }];

        NSError *error = nil;
        id hid = ((id (*)(id, SEL, id, NSError **))objc_msgSend)([NSClassFromString(@"SimulatorKit.SimDeviceLegacyHIDClient") alloc],
                                                                NSSelectorFromString(@"initWithDevice:error:"), dev, &error);
        say(@"hid client %@ %@", hid, error ?: @"");
        TouchView *touch = [[TouchView alloc] initWithFrame:content.bounds];
        touch.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
        touch.hid = hid;
        [content addSubview:touch];

        [window makeFirstResponder:touch];
        if (front) { [window makeKeyAndOrderFront:nil]; [app activateIgnoringOtherApps:YES]; } else { [window orderBack:nil]; }

        if (opts[@"tap"]) {
            NSArray *xy = [opts[@"tap"] componentsSeparatedByString:@","];
            NSPoint p = NSMakePoint([xy[0] doubleValue], [xy[1] doubleValue]);
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC), dispatch_get_main_queue(), ^{
                [touch send:NSEventTypeLeftMouseDown at:p];
                dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 80 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{ [touch send:NSEventTypeLeftMouseUp at:p]; });
            });
        }
        if (opts[@"home"]) {
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC), dispatch_get_main_queue(), ^{ [touch home]; });
        }
        if (opts[@"swipe"]) {
            NSArray *v = [opts[@"swipe"] componentsSeparatedByString:@","];
            NSPoint a = NSMakePoint([v[0] doubleValue], [v[1] doubleValue]), b = NSMakePoint([v[2] doubleValue], [v[3] doubleValue]);
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC), dispatch_get_main_queue(), ^{
                [touch send:NSEventTypeLeftMouseDown at:a];
                for (int i = 1; i <= 20; i++) {
                    NSPoint p = NSMakePoint(a.x + (b.x - a.x) * i / 20, a.y + (b.y - a.y) * i / 20);
                    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, i * 16 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
                        [touch send:i == 20 ? NSEventTypeLeftMouseUp : NSEventTypeLeftMouseDragged at:p];
                    });
                }
            });
        }
        if (opts[@"capture"]) {
            NSString *path = opts[@"capture"];
            double delay = opts[@"capture-after"] ? [opts[@"capture-after"] doubleValue] : 4;
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(delay * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{ capture(window, path); });
        }
        if (opts[@"exit-after"]) {
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)([opts[@"exit-after"] doubleValue] * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{ exit(0); });
        }
        [app run];
    }
}
