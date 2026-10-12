#ifndef KANNA_SYNTAX_H
#define KANNA_SYNTAX_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    uint32_t start;
    uint32_t end;
    uint32_t capture;
} KannaSyntaxSpan;

typedef struct {
    uint32_t first;
    uint32_t last;
} KannaSyntaxScope;

typedef struct {
    KannaSyntaxSpan *spans;
    size_t span_count;
    KannaSyntaxScope *scopes;
    size_t scope_count;
} KannaSyntaxResult;

const char *kanna_syntax_language(const char *file_name, const char *first_line);
size_t kanna_syntax_capture_count(void);
const char *kanna_syntax_capture_name(size_t index);
KannaSyntaxResult *kanna_syntax_highlight(const char *language, const uint8_t *text, size_t length);
void kanna_syntax_free(KannaSyntaxResult *result);

#endif
