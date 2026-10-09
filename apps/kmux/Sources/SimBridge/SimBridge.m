#import "SimBridge.h"
#import <dlfcn.h>
#import <objc/message.h>

// Signatures read from SimulatorKit's disassembly (see spikes/ios-sim):
// IndigoHIDMessageForMouseNSEvent(point, point2, target, eventType, size, edge)
// and IndigoHIDMessageForButton(button, direction, target).
typedef void *(*IndigoMouseFn)(CGPoint *point, CGPoint *point2, uint32_t target, NSEventType type, NSSize size, uint32_t edge);
typedef void *(*IndigoButtonFn)(uint32_t button, uint32_t direction, uint32_t target);

static const uint32_t kTouchScreen = 0x32;
static const uint32_t kButtons = 0x33;
static const uint32_t kHomeButton = 0;

static NSError *simError(NSString *message) {
    return [NSError errorWithDomain:@"kmux.ios" code:1 userInfo:@{NSLocalizedDescriptionKey: message}];
}

static id send0(id target, NSString *selector) {
    SEL sel = NSSelectorFromString(selector);
    return [target respondsToSelector:sel] ? ((id (*)(id, SEL))objc_msgSend)(target, sel) : nil;
}

@implementation KSimScreen {
    id _hid;
    id _display;
    NSUUID *_callbacks;
    __weak CALayer *_layer;
    IndigoMouseFn _mouse;
    IndigoButtonFn _button;
}

/// Loads the private frameworks once; nil on success, else what went wrong.
static NSString *load(NSString *developerDir) {
    static NSString *failure;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        NSString *core = @"/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator";
        if (!dlopen(core.fileSystemRepresentation, RTLD_NOW)) { failure = [NSString stringWithFormat:@"can't load CoreSimulator: %s", dlerror()]; return; }
        // SimulatorKit is in Contents/SharedFrameworks since Xcode 27, and in
        // Contents/Developer/Library/PrivateFrameworks before that.
        NSString *shared = [developerDir.stringByDeletingLastPathComponent stringByAppendingPathComponent:@"SharedFrameworks/SimulatorKit.framework/SimulatorKit"];
        NSString *private = [developerDir stringByAppendingPathComponent:@"Library/PrivateFrameworks/SimulatorKit.framework/SimulatorKit"];
        for (NSString *kit in @[shared, private]) {
            if (dlopen(kit.fileSystemRepresentation, RTLD_NOW)) return;
        }
        failure = [NSString stringWithFormat:@"can't find SimulatorKit in %@", developerDir.stringByDeletingLastPathComponent];
    });
    return failure;
}

- (nullable instancetype)initWithUDID:(NSString *)udid developerDir:(NSString *)developerDir layer:(CALayer *)layer error:(NSError **)error {
    if (!(self = [super init])) return nil;
    NSString *failure = load(developerDir);
    if (failure) { if (error) *error = simError(failure); return nil; }
    _mouse = (IndigoMouseFn)dlsym(RTLD_DEFAULT, "IndigoHIDMessageForMouseNSEvent");
    _button = (IndigoButtonFn)dlsym(RTLD_DEFAULT, "IndigoHIDMessageForButton");
    Class contextClass = NSClassFromString(@"SimServiceContext");
    Class hidClass = NSClassFromString(@"SimulatorKit.SimDeviceLegacyHIDClient");
    if (!_mouse || !_button || !contextClass || !hidClass) {
        if (error) *error = simError(@"this Xcode's simulator frameworks have changed: kmux can't show the screen");
        return nil;
    }

    NSError *inner = nil;
    id context = ((id (*)(id, SEL, id, NSError **))objc_msgSend)(contextClass, NSSelectorFromString(@"sharedServiceContextForDeveloperDir:error:"), developerDir, &inner);
    id set = context ? ((id (*)(id, SEL, NSError **))objc_msgSend)(context, NSSelectorFromString(@"defaultDeviceSetWithError:"), &inner) : nil;
    id device = nil;
    for (id d in [set valueForKey:@"devices"]) {
        if ([[[d valueForKey:@"UDID"] UUIDString] isEqualToString:udid]) { device = d; break; }
    }
    if (!device) { if (error) *error = simError(inner.localizedDescription ?: [NSString stringWithFormat:@"no simulator device %@", udid]); return nil; }

    // The main display: the port whose descriptor has a framebuffer and display class 0.
    for (id port in send0(send0(device, @"io"), @"ioPorts")) {
        id descriptor = send0(port, @"descriptor");
        if (![descriptor respondsToSelector:NSSelectorFromString(@"framebufferSurface")]) continue;
        id state = send0(descriptor, @"state");
        SEL classSel = NSSelectorFromString(@"displayClass");
        if (![state respondsToSelector:classSel]) continue;
        if (((unsigned short (*)(id, SEL))objc_msgSend)(state, classSel) == 0) { _display = descriptor; break; }
    }
    if (!_display) { if (error) *error = simError(@"the simulator has no screen yet (is it booted?)"); return nil; }

    _hid = ((id (*)(id, SEL, id, NSError **))objc_msgSend)([hidClass alloc], NSSelectorFromString(@"initWithDevice:error:"), device, &inner);
    if (!_hid) { if (error) *error = simError(inner.localizedDescription ?: @"can't send touches to the simulator"); return nil; }

    _layer = layer;
    _callbacks = [NSUUID UUID];
    [self showSurface];
    __weak KSimScreen *weakSelf = self;
    ((void (*)(id, SEL, id, id))objc_msgSend)(_display, NSSelectorFromString(@"registerCallbackWithUUID:ioSurfacesChangeCallback:"), _callbacks,
        ^(id a, id b) { dispatch_async(dispatch_get_main_queue(), ^{ [weakSelf showSurface]; }); });
    ((void (*)(id, SEL, id, id))objc_msgSend)(_display, NSSelectorFromString(@"registerCallbackWithUUID:damageRectanglesCallback:"), _callbacks,
        ^(NSArray *rects) { dispatch_async(dispatch_get_main_queue(), ^{ [weakSelf frameChanged]; }); });
    return self;
}

- (void)showSurface {
    id surface = send0(_display, @"framebufferSurface");
    CGSize old = _pixelSize;
    _pixelSize = surface ? CGSizeMake(IOSurfaceGetWidth((__bridge IOSurfaceRef)surface), IOSurfaceGetHeight((__bridge IOSurfaceRef)surface)) : CGSizeZero;
    [CATransaction begin];
    [CATransaction setDisableActions:YES];
    _layer.contents = surface;
    [CATransaction commit];
    if (!CGSizeEqualToSize(old, _pixelSize) && self.onResize) self.onResize();
}

/// The surface is the same; its pixels changed.
- (void)frameChanged {
    CALayer *layer = _layer;
    if (layer) ((void (*)(id, SEL))objc_msgSend)(layer, NSSelectorFromString(@"setContentsChanged"));
}

- (void)deliver:(void *)message {
    if (!message || !_hid) return;
    ((void (*)(id, SEL, void *, BOOL, id, id))objc_msgSend)(_hid, NSSelectorFromString(@"sendWithMessage:freeWhenDone:completionQueue:completion:"),
        message, YES, dispatch_get_main_queue(), ^(NSError *error) {
            if (error) NSLog(@"kmux: simulator input failed: %@", error);
        });
}

- (void)touch:(NSEventType)type at:(CGPoint)fraction {
    CGPoint point = CGPointMake(MIN(MAX(fraction.x, 0), 1), MIN(MAX(fraction.y, 0), 1));
    [self deliver:_mouse(&point, NULL, kTouchScreen, type, NSMakeSize(1, 1), 0)];
}

- (void)pressHome {
    [self deliver:_button(kHomeButton, 1, kButtons)];
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 60 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
        [self deliver:self->_button(kHomeButton, 2, kButtons)];
    });
}

- (void)stop {
    for (NSString *selector in @[@"unregisterIOSurfacesChangeCallbackWithUUID:", @"unregisterDamageRectanglesCallbackWithUUID:"]) {
        SEL sel = NSSelectorFromString(selector);
        if (_callbacks && [_display respondsToSelector:sel]) ((void (*)(id, SEL, id))objc_msgSend)(_display, sel, _callbacks);
    }
    _callbacks = nil;
    _display = nil;
    _hid = nil;
}

- (void)dealloc { [self stop]; }

@end
