"""Optional native oracle for the backend's partial-title regression."""

import base64
import sys

from kitty.fast_data_types import Screen, set_options
from kitty.options.types import defaults


class Callbacks:
    title = ""

    def title_changed(self, title):
        self.title = bytes(title).decode("utf-8")

    def __getattr__(self, name):
        return lambda *args, **kwargs: None


def parse(target, data):
    # Same native parser entry points used by kitty_tests.parse_bytes.
    data = memoryview(data)
    while data:
        dest = target.test_create_write_buffer()
        count = target.test_commit_write_buffer(data, dest)
        data = data[count:]
        target.test_parse_written_data(None)


def check(condition, message):
    # Kitty's bundled Python runs with optimization, which removes assert.
    if not condition:
        raise RuntimeError(message)


set_options(defaults)
callbacks = Callbacks()
target = Screen(callbacks, 24, 80, 0, 10, 20, 0, callbacks)

# Negative control: mere cleanup byte presence is insufficient inside OSC.
parse(target, b"\x1b[?1049h\x1b]0;ti\x1b[?1049l")
check(target.is_using_alternate_linebuf(), "negative control did not keep OSC open")
parse(target, b"\x1b\\\x1b[?1049l")
check(not target.is_using_alternate_linebuf(), "ST control did not restore main screen")

check(len(sys.argv) > 1 and (len(sys.argv) - 1) % 3 == 0, "missing parser cases")
for alternate, title, data in zip(sys.argv[1::3], sys.argv[2::3], sys.argv[3::3]):
    callbacks = Callbacks()
    target = Screen(callbacks, 24, 80, 0, 10, 20, 0, callbacks)
    parse(target, base64.b64decode(data))
    check(
        target.is_using_alternate_linebuf() == (alternate == "true"),
        "unexpected alternate screen state",
    )
    check(callbacks.title == title, repr(callbacks.title))
    parse(target, b"\x1b[HPARSER_READY")
    check(
        str(target.line(0)).startswith("PARSER_READY"), "parser still inside a string"
    )
print("ARBORUI_KITTY_PARSER_OK")
