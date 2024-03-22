import cv2
import numpy as np
import win32gui, win32ui
from ctypes import windll

def grab_screen(screen_name=None, region=None):

    if screen_name:
        hwin = win32gui.FindWindow(None, screen_name)
        if hwin == 0:
            raise ValueError("Window with the given name not found.")
    else:
        hwin = win32gui.GetDesktopWindow()

    if region:
        left, top, x2, y2 = region
        width = x2 - left + 1
        height = y2 - top + 1
    else:
        left, top, right, bottom = win32gui.GetWindowRect(hwin)
        width = right - left
        height = bottom - top

    hwindc = win32gui.GetWindowDC(hwin)
    srcdc = win32ui.CreateDCFromHandle(hwindc)
    memdc = srcdc.CreateCompatibleDC()
    bmp = win32ui.CreateBitmap()
    bmp.CreateCompatibleBitmap(srcdc, width, height)
    memdc.SelectObject(bmp)

    windll.user32.PrintWindow(hwin, memdc.GetSafeHdc(), 2)

    signedIntsArray = bmp.GetBitmapBits(True)
    img = np.fromstring(signedIntsArray, dtype='uint8')
    img.shape = (height, width, 4)

    win32gui.DeleteObject(bmp.GetHandle())
    memdc.DeleteDC()
    srcdc.DeleteDC()
    win32gui.ReleaseDC(hwin, hwindc)
 

    return cv2.cvtColor(img, cv2.COLOR_BGRA2RGB)
