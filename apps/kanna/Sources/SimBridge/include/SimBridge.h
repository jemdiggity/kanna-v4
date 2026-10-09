#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>

NS_ASSUME_NONNULL_BEGIN

@interface KSimScreen : NSObject

- (nullable instancetype)initWithUDID:(NSString *)udid developerDir:(NSString *)developerDir
                                layer:(CALayer *)layer error:(NSError **)error;

@property(readonly) CGSize pixelSize;
@property(nullable, copy) void (^onResize)(void);

- (void)touch:(NSEventType)type at:(CGPoint)fraction;
- (void)pressHome;
- (void)stop;

@end

NS_ASSUME_NONNULL_END
