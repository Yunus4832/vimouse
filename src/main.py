import pyautogui
from controller.appcontroller import AppController
import ctypes

pyautogui.FAILSAFE = False
pyautogui.PAUSE = 0

ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID("Vimouse")

if __name__ == '__main__':
    controller = AppController()
    controller.start()
