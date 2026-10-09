// Prints the device's IO ports and what their descriptors can do.
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
#import <objc/message.h>
#import <dlfcn.h>
static void methods(id o) {
    for (Class c = object_getClass(o); c && c != [NSObject class]; c = class_getSuperclass(c)) {
        printf("    class %s\n", class_getName(c));
        unsigned n; Method *m = class_copyMethodList(c, &n);
        for (unsigned i = 0; i < n; i++) printf("      - %s %s\n", sel_getName(method_getName(m[i])), method_getTypeEncoding(m[i]));
        unsigned pn; Protocol * __unsafe_unretained *ps = class_copyProtocolList(c, &pn);
        for (unsigned i = 0; i < pn; i++) printf("      <%s>\n", protocol_getName(ps[i]));
    }
}
int main(int argc, char **argv) { @autoreleasepool {
    dlopen("/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator", RTLD_NOW);
    dlopen("/Applications/Xcode.app/Contents/Developer/Library/PrivateFrameworks/SimulatorKit.framework/SimulatorKit", RTLD_NOW);
    NSError *e = nil;
    id ctx = ((id (*)(id, SEL, id, NSError **))objc_msgSend)(NSClassFromString(@"SimServiceContext"), NSSelectorFromString(@"sharedServiceContextForDeveloperDir:error:"), @"/Applications/Xcode.app/Contents/Developer", &e);
    id set = ((id (*)(id, SEL, NSError **))objc_msgSend)(ctx, NSSelectorFromString(@"defaultDeviceSetWithError:"), &e);
    for (id d in [set valueForKey:@"devices"]) {
        if (![[[d valueForKey:@"UDID"] UUIDString] isEqualToString:@(argv[1])]) continue;
        id io = [d valueForKey:@"io"];
        printf("io %s\n", [[io description] UTF8String]);
        for (id port in [io valueForKey:@"ioPorts"]) {
            printf("  port %s\n", class_getName(object_getClass(port)));
            id desc = [port valueForKey:@"descriptor"];
            printf("  descriptor %s\n", [[desc description] UTF8String]);
            methods(desc);
        }
    }
}}
