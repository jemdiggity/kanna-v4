#ifndef KANNA_LAYOUT_H
#define KANNA_LAYOUT_H

#include <stdbool.h>

typedef struct KannaLayout KannaLayout;

KannaLayout *kanna_layout_open(const char *state_path, bool fresh, char **problem);
char *kanna_layout_call(KannaLayout *layout, const char *request);
void kanna_layout_free_string(char *string);
void kanna_layout_close(KannaLayout *layout);

#endif
