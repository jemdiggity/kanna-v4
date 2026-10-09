#ifndef KANNA_MERMAN_H
#define KANNA_MERMAN_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    const char *text;
    size_t text_len;
    const char *font_family;
    size_t font_family_len;
    double font_size;
    int32_t bold;
    int32_t italic;
    double max_width;
    int32_t html_like;
    int32_t kind;
} kanna_merman_measure_request;

enum {
    KANNA_MEASURE_METRICS = 0,
    KANNA_MEASURE_WIDTH = 1,
    KANNA_MEASURE_HEIGHT = 2,
    KANNA_MEASURE_EXTENTS = 3,
    KANNA_MEASURE_WRAPPED_RAW = 4,
};

typedef struct {
    double width, height;
    uint32_t line_count;
    double length;
    double left, right;
    double raw_width;
} kanna_merman_measure_result;

typedef int32_t (*kanna_merman_measure_fn)(const kanna_merman_measure_request *request,
                                          kanna_merman_measure_result *out, void *context);

char *kanna_merman_layout(const char *source, size_t len, kanna_merman_measure_fn measure, void *context);

void kanna_merman_free(char *json);

#endif
