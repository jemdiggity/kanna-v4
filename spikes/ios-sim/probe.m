// Lists the private Simulator classes and methods this Xcode has.
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
#import <dlfcn.h>

static void dump(NSString *name) {
    Class c = NSClassFromString(name);
    if (!c) { printf("%s: missing\n", name.UTF8String); return; }
    printf("== %s : %s\n", name.UTF8String, class_getName(class_getSuperclass(c)));
    unsigned n; Method *m = class_copyMethodList(c, &n);
    for (unsigned i = 0; i < n; i++) printf("  - %s\n", sel_getName(method_getName(m[i])));
    Ivar *iv = class_copyIvarList(c, &n);
    for (unsigned i = 0; i < n; i++) printf("  ivar %s %s\n", ivar_getName(iv[i]), ivar_getTypeEncoding(iv[i]) ?: "");
    m = class_copyMethodList(object_getClass(c), &n);
    for (unsigned i = 0; i < n; i++) printf("  + %s\n", sel_getName(method_getName(m[i])));
}

int main(int argc, char **argv) {
    @autoreleasepool {
        dlopen("/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator", RTLD_NOW);
        dlopen("/Applications/Xcode.app/Contents/Developer/Library/PrivateFrameworks/SimulatorKit.framework/SimulatorKit", RTLD_NOW);
        if (argc > 1) { for (int i = 1; i < argc; i++) dump(@(argv[i])); return 0; }
        unsigned n; const char **names = objc_copyClassNamesForImage("/Applications/Xcode.app/Contents/Developer/Library/PrivateFrameworks/SimulatorKit.framework/Versions/A/SimulatorKit", &n);
        for (unsigned i = 0; i < n; i++) printf("%s\n", names[i]);
    }
}
