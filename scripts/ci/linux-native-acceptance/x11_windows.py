#!/usr/bin/env python3
# created: 2026-10-22 | branch: ci/k1l-linux-native-acceptance | author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
# status: active (K1-L, SCL-S14.6). X11 helper for scripts/ci/linux-native-acceptance.sh.
#
#   x11_windows.py list  <pid>   print "0x<id> <title>" for each top-level window of <pid>
#   x11_windows.py close <pid>   ask each such window to close, the way a window manager's close
#                                button does: a WM_PROTOCOLS / WM_DELETE_WINDOW client message.
#                                Exit 0 if at least one window was asked, 3 if none was found.
#
# Only for the private Xvfb display the harness starts inside its namespace: it refuses to run
# unless DISPLAY is the display the harness exported in K1L_PRIVATE_DISPLAY.
# Standard library only (ctypes over libX11), so it runs on a stock runner.
import ctypes
import ctypes.util
import os
import sys

Window = ctypes.c_ulong
Atom = ctypes.c_ulong


class XClientMessageEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("window", Window),
        ("message_type", Atom),
        ("format", ctypes.c_int),
        ("l", ctypes.c_long * 5),
    ]


class XEvent(ctypes.Union):
    _fields_ = [("xclient", XClientMessageEvent), ("pad", ctypes.c_long * 24)]


CLIENT_MESSAGE = 33
NO_EVENT_MASK = 0
XA_CARDINAL = 6
ANY_PROPERTY_TYPE = 0


def load():
    name = ctypes.util.find_library("X11") or "libX11.so.6"
    x = ctypes.CDLL(name)
    x.XOpenDisplay.restype = ctypes.c_void_p
    x.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x.XDefaultRootWindow.restype = Window
    x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
    x.XInternAtom.restype = Atom
    x.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
    x.XQueryTree.argtypes = [
        ctypes.c_void_p, Window, ctypes.POINTER(Window), ctypes.POINTER(Window),
        ctypes.POINTER(ctypes.POINTER(Window)), ctypes.POINTER(ctypes.c_uint),
    ]
    x.XGetWindowProperty.argtypes = [
        ctypes.c_void_p, Window, Atom, ctypes.c_long, ctypes.c_long, ctypes.c_int, Atom,
        ctypes.POINTER(Atom), ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_ulong),
        ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_void_p),
    ]
    x.XFree.argtypes = [ctypes.c_void_p]
    x.XSendEvent.argtypes = [ctypes.c_void_p, Window, ctypes.c_int, ctypes.c_long,
                             ctypes.POINTER(XEvent)]
    x.XFlush.argtypes = [ctypes.c_void_p]
    x.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
    x.XCloseDisplay.argtypes = [ctypes.c_void_p]
    return x


def prop(x, dpy, win, atom, kind):
    """Return the property as a list of longs (format 32) or bytes (format 8), or None."""
    at = Atom()
    fmt = ctypes.c_int()
    n = ctypes.c_ulong()
    after = ctypes.c_ulong()
    data = ctypes.c_void_p()
    rc = x.XGetWindowProperty(dpy, win, atom, 0, 1024, 0, kind, ctypes.byref(at),
                              ctypes.byref(fmt), ctypes.byref(n), ctypes.byref(after),
                              ctypes.byref(data))
    if rc != 0 or not data.value:
        return None
    try:
        if fmt.value == 32:
            arr = ctypes.cast(data, ctypes.POINTER(ctypes.c_long))
            return [arr[i] for i in range(n.value)]
        if fmt.value == 8:
            return ctypes.string_at(data, n.value)
        return None
    finally:
        x.XFree(data)


def children(x, dpy, win):
    root = Window()
    parent = Window()
    kids = ctypes.POINTER(Window)()
    n = ctypes.c_uint()
    if not x.XQueryTree(dpy, win, ctypes.byref(root), ctypes.byref(parent), ctypes.byref(kids),
                        ctypes.byref(n)):
        return []
    try:
        return [kids[i] for i in range(n.value)]
    finally:
        if kids:
            x.XFree(kids)


def windows_of(x, dpy, pid):
    net_wm_pid = x.XInternAtom(dpy, b"_NET_WM_PID", 0)
    net_wm_name = x.XInternAtom(dpy, b"_NET_WM_NAME", 0)
    utf8 = x.XInternAtom(dpy, b"UTF8_STRING", 0)
    out = []
    stack = [x.XDefaultRootWindow(dpy)]
    while stack:
        w = stack.pop()
        p = prop(x, dpy, w, net_wm_pid, XA_CARDINAL)
        if p and p[0] == pid:
            title = prop(x, dpy, w, net_wm_name, utf8) or b""
            out.append((w, title.decode("utf-8", "replace")))
        stack.extend(children(x, dpy, w))
    return out


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ("list", "close"):
        print(__doc__ or "usage: x11_windows.py list|close <pid>", file=sys.stderr)
        return 2
    private = os.environ.get("K1L_PRIVATE_DISPLAY", "")
    display = os.environ.get("DISPLAY", "")
    if not private or display != private:
        print(f"refusing: DISPLAY={display!r} is not the harness display {private!r}",
              file=sys.stderr)
        return 2
    pid = int(sys.argv[2])
    x = load()
    dpy = x.XOpenDisplay(display.encode())
    if not dpy:
        print(f"cannot open display {display}", file=sys.stderr)
        return 2
    try:
        wins = windows_of(x, dpy, pid)
        if sys.argv[1] == "list":
            for w, t in wins:
                print(f"0x{w:x} {t}")
            return 0 if wins else 3
        wm_protocols = x.XInternAtom(dpy, b"WM_PROTOCOLS", 0)
        wm_delete = x.XInternAtom(dpy, b"WM_DELETE_WINDOW", 0)
        asked = 0
        for w, t in wins:
            protos = prop(x, dpy, w, wm_protocols, ANY_PROPERTY_TYPE) or []
            if wm_delete not in protos:
                continue
            ev = XEvent()
            ev.xclient.type = CLIENT_MESSAGE
            ev.xclient.window = w
            ev.xclient.message_type = wm_protocols
            ev.xclient.format = 32
            ev.xclient.l[0] = wm_delete
            ev.xclient.l[1] = 0  # CurrentTime
            x.XSendEvent(dpy, w, 0, NO_EVENT_MASK, ctypes.byref(ev))
            print(f"asked 0x{w:x} {t} to close")
            asked += 1
        x.XSync(dpy, 0)
        return 0 if asked else 3
    finally:
        x.XCloseDisplay(dpy)


if __name__ == "__main__":
    sys.exit(main())
