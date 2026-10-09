// Prints the Simulator display/IO protocols and their methods.
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
#import <dlfcn.h>
int main(void) { @autoreleasepool {
    dlopen("/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator", RTLD_NOW);
    dlopen("/Applications/Xcode.app/Contents/Developer/Library/PrivateFrameworks/SimulatorKit.framework/SimulatorKit", RTLD_NOW);
    unsigned n; Protocol * __unsafe_unretained *ps = objc_copyProtocolList(&n);
    for (unsigned i = 0; i < n; i++) {
        NSString *name = @(protocol_getName(ps[i]));
        if (!([name containsString:@"SimDisplay"] || [name containsString:@"SimDeviceIOPort"] || [name containsString:@"IOSurface"] || [name containsString:@"SimDeviceIOProtocol"])) continue;
        printf("@protocol %s\n", name.UTF8String);
        for (int req = 1; req >= 0; req--) for (int inst = 1; inst >= 0; inst--) {
            unsigned mn; struct objc_method_description *ms = protocol_copyMethodDescriptionList(ps[i], req, inst, &mn);
            for (unsigned j = 0; j < mn; j++) printf("  %s%s %s\n", inst ? "-" : "+", req ? "" : " (optional)", sel_getName(ms[j].name));
        }
    }
}}
