#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>

NS_ASSUME_NONNULL_BEGIN

/// A booted iOS Simulator device's screen and touch input, through Xcode's
/// private CoreSimulator and SimulatorKit frameworks (see spikes/ios-sim).
/// Booting, installing and launching go through `simctl` instead.
@interface KSimScreen : NSObject

/// Shows the device's main display in `layer` and keeps it updated. Fails
/// with a readable error when the device isn't found or booted, or when this
/// Xcode's private frameworks have changed shape.
- (nullable instancetype)initWithUDID:(NSString *)udid developerDir:(NSString *)developerDir
                                layer:(CALayer *)layer error:(NSError **)error;

/// The screen in pixels (zero until the first frame).
@property(readonly) CGSize pixelSize;
/// Called on the main queue when the screen's size changes (e.g. rotation).
@property(nullable, copy) void (^onResize)(void);

/// A touch: began (left mouse down), moved (dragged) or ended (up), at a
/// point given as a fraction of the screen from its top left.
- (void)touch:(NSEventType)type at:(CGPoint)fraction;
/// Presses and releases the Home button.
- (void)pressHome;
/// Stops updating the layer.
- (void)stop;

@end

NS_ASSUME_NONNULL_END
