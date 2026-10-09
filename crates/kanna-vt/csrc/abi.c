#define GHOSTTY_STATIC
#include <ghostty/vt.h>
#include <stddef.h>

static const long long kanna_vt_abi_values[] = {
    sizeof(GhosttyResult),
    GHOSTTY_SUCCESS,
    GHOSTTY_OUT_OF_MEMORY,
    GHOSTTY_INVALID_VALUE,
    GHOSTTY_OUT_OF_SPACE,
    GHOSTTY_NO_VALUE,
    sizeof(GhosttyString),
    offsetof(GhosttyString, len),
    GHOSTTY_FORMATTER_FORMAT_PLAIN,
    GHOSTTY_TERMINAL_OPT_USERDATA,
    GHOSTTY_TERMINAL_OPT_WRITE_PTY,
    GHOSTTY_TERMINAL_OPT_SIZE,
    GHOSTTY_TERMINAL_OPT_DEVICE_ATTRIBUTES,
    GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,
    GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES,
    GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES,
    GHOSTTY_TERMINAL_DATA_COLS,
    GHOSTTY_TERMINAL_DATA_ROWS,
    GHOSTTY_TERMINAL_DATA_CURSOR_X,
    GHOSTTY_TERMINAL_DATA_CURSOR_Y,
    GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN,
    GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE,
    GHOSTTY_TERMINAL_DATA_TITLE,
    GHOSTTY_TERMINAL_DATA_PWD,
    GHOSTTY_TERMINAL_DATA_TOTAL_ROWS,
    GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS,
    GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_BYTES,
    GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_LINES,
    sizeof(GhosttyTerminalScreen),
    GHOSTTY_TERMINAL_SCREEN_ALTERNATE,
    GHOSTTY_SNAPSHOT_DECODER_OPT_MAX_CONTINUATION_BYTES,
    GHOSTTY_SNAPSHOT_DECODER_OPT_RETAIN_CONTINUATION,
    GHOSTTY_POINT_TAG_SCREEN,
    sizeof(GhosttyPointCoordinate),
    offsetof(GhosttyPointCoordinate, y),
    sizeof(GhosttyPointValue),
    sizeof(GhosttyPoint),
    offsetof(GhosttyPoint, value),
    sizeof(GhosttyGridRef),
    offsetof(GhosttyGridRef, node),
    offsetof(GhosttyGridRef, x),
    offsetof(GhosttyGridRef, y),
    sizeof(GhosttySelection),
    offsetof(GhosttySelection, start),
    offsetof(GhosttySelection, end),
    offsetof(GhosttySelection, rectangle),
    sizeof(GhosttyTerminalSelectionFormatOptions),
    offsetof(GhosttyTerminalSelectionFormatOptions, emit),
    offsetof(GhosttyTerminalSelectionFormatOptions, unwrap),
    offsetof(GhosttyTerminalSelectionFormatOptions, trim),
    offsetof(GhosttyTerminalSelectionFormatOptions, selection),
    sizeof(GhosttyDeviceAttributesPrimary),
    offsetof(GhosttyDeviceAttributesPrimary, features),
    offsetof(GhosttyDeviceAttributesPrimary, num_features),
    sizeof(GhosttyDeviceAttributesSecondary),
    offsetof(GhosttyDeviceAttributesSecondary, firmware_version),
    offsetof(GhosttyDeviceAttributesSecondary, rom_cartridge),
    sizeof(GhosttyDeviceAttributesTertiary),
    sizeof(GhosttyDeviceAttributes),
    offsetof(GhosttyDeviceAttributes, secondary),
    offsetof(GhosttyDeviceAttributes, tertiary),
    sizeof(GhosttySizeReportSize),
    offsetof(GhosttySizeReportSize, columns),
    offsetof(GhosttySizeReportSize, cell_width),
    offsetof(GhosttySizeReportSize, cell_height),
};

#define SIGNATURE(name, ret, ...) static ret (*const check_##name)(__VA_ARGS__) __attribute__((used)) = name
SIGNATURE(ghostty_free, void, const GhosttyAllocator *, uint8_t *, size_t);
SIGNATURE(ghostty_terminal_new, GhosttyResult, const GhosttyAllocator *, GhosttyTerminal *, uint16_t, uint16_t);
SIGNATURE(ghostty_terminal_free, void, GhosttyTerminal);
SIGNATURE(ghostty_terminal_resize, GhosttyResult, GhosttyTerminal, uint16_t, uint16_t, uint32_t, uint32_t);
SIGNATURE(ghostty_terminal_set, GhosttyResult, GhosttyTerminal, GhosttyTerminalOption, const void *);
SIGNATURE(ghostty_terminal_vt_write, void, GhosttyTerminal, const uint8_t *, size_t);
SIGNATURE(ghostty_terminal_get, GhosttyResult, GhosttyTerminal, GhosttyTerminalData, void *);
SIGNATURE(ghostty_terminal_grid_ref, GhosttyResult, GhosttyTerminal, GhosttyPoint, GhosttyGridRef *);
SIGNATURE(ghostty_terminal_selection_format_alloc, GhosttyResult, GhosttyTerminal, const GhosttyAllocator *,
          GhosttyTerminalSelectionFormatOptions, uint8_t **, size_t *);
SIGNATURE(ghostty_snapshot_encode_alloc, GhosttyResult, GhosttyTerminal, const GhosttyAllocator *, uint8_t **, size_t *);
SIGNATURE(ghostty_snapshot_decoder_new_buf, GhosttyResult, const GhosttyAllocator *, GhosttySnapshotDecoder *,
          const uint8_t *, size_t);
SIGNATURE(ghostty_snapshot_decoder_set, GhosttyResult, GhosttySnapshotDecoder, GhosttySnapshotDecoderOption, const void *);
SIGNATURE(ghostty_snapshot_decoder_decode, GhosttyResult, GhosttySnapshotDecoder, GhosttyTerminal *);
SIGNATURE(ghostty_snapshot_decoder_free, void, GhosttySnapshotDecoder);
static void (*const check_write_pty)(GhosttyTerminal, void *, const uint8_t *, size_t) __attribute__((used)) =
    (GhosttyTerminalWritePtyFn)0;
static bool (*const check_device_attributes)(GhosttyTerminal, void *, GhosttyDeviceAttributes *) __attribute__((used)) =
    (GhosttyTerminalDeviceAttributesFn)0;
static bool (*const check_size)(GhosttyTerminal, void *, GhosttySizeReportSize *) __attribute__((used)) =
    (GhosttyTerminalSizeFn)0;

size_t kanna_vt_abi(long long *out, size_t cap) {
  size_t n = sizeof kanna_vt_abi_values / sizeof kanna_vt_abi_values[0];
  for (size_t i = 0; i < n && i < cap; i++) out[i] = kanna_vt_abi_values[i];
  return n;
}
