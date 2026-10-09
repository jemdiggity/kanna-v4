// kmux's bridge to merman: Mermaid source in, layout JSON out, with text
// measured by the host (Core Text) so labels fit the boxes kmux draws.
#ifndef KMUX_MERMAN_H
#define KMUX_MERMAN_H

#include <stddef.h>
#include <stdint.h>

// One text measurement asked for by merman's layout.
typedef struct {
    const char *text;      // UTF-8, `text_len` bytes, not NUL-terminated
    size_t text_len;
    const char *font_family; // may be NULL
    size_t font_family_len;
    double font_size;      // CSS px
    int32_t bold;
    int32_t italic;
    double max_width;      // wrap width, or < 0 for none
    int32_t html_like;     // 1: HTML label (line height 1.5), 0: SVG text (1.1)
    int32_t kind;          // what to return: see KMUX_MEASURE_*
} kmux_merman_measure_request;

enum {
    KMUX_MEASURE_METRICS = 0, // width, height, line_count (wrapped to max_width)
    KMUX_MEASURE_WIDTH = 1,   // length = width of the text on one line
    KMUX_MEASURE_HEIGHT = 2,  // length = height of the text
    KMUX_MEASURE_EXTENTS = 3, // left, right of the text on one line
    KMUX_MEASURE_WRAPPED_RAW = 4, // metrics plus raw_width (unwrapped width)
};

typedef struct {
    double width, height;
    uint32_t line_count;
    double length;
    double left, right;
    double raw_width;
} kmux_merman_measure_result;

// Returns 1 with `out` filled, or 0 to let merman measure it itself.
typedef int32_t (*kmux_merman_measure_fn)(const kmux_merman_measure_request *request,
                                          kmux_merman_measure_result *out, void *context);

// Lays out `source` (UTF-8, `len` bytes). Returns a NUL-terminated JSON string
// to free with kmux_merman_free: {"meta","semantic","layout"} from merman, or
// {"error": "..."}. `measure` may be NULL (merman's own metrics).
char *kmux_merman_layout(const char *source, size_t len, kmux_merman_measure_fn measure, void *context);

void kmux_merman_free(char *json);

#endif
