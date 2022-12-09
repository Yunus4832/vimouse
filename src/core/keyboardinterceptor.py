# -*- coding: UTF-8 -*-
from pynput import keyboard

from utils.keycodeutil import KeyCodeUtil
from threading import Thread
from queue import Queue


class KeyboardInterceptor(Thread):
    VK_A = 65
    VK_C = 67
    VK_D = 68
    VK_F = 70
    VK_H = 72
    VK_I = 73
    VK_J = 74
    VK_K = 75
    VK_L = 76
    VK_O = 79
    VK_P = 80
    VK_R = 82
    VK_U = 85
    VK_V = 86
    VK_W = 87
    VK_Y = 89
    VK_SPACE = 32
    Vk_ALT = 18
    VK_LSHIFT = 160
    VK_OEM_1 = 186
    MSG_PRESS = 256
    MSG_PRESS_WITH_ALT = 260
    MSG_RELEASE_WITH_ALT = 261
    MSG_RELEASE = 257

    # 持续生效按键
    continuousSet = {'h', 'j', 'k', 'l', 'r', 'f', 'space', 'shift_l'}
    # 按下触发键位
    pressTriggerSet = {'a', 'v', 'y', 'c', 'p', 'u'}
    # 首尾触发键位
    headTailTriggerSet = {'i', 'o', 'e', 'w', 'd'}
    # 有效按键
    usedKeySet = {VK_A, VK_C, VK_D, VK_F, VK_H, VK_I, VK_J, VK_K, VK_L, VK_O, VK_P, VK_R,
                  VK_U, VK_V, VK_W, VK_Y, VK_SPACE, Vk_ALT, VK_LSHIFT, VK_OEM_1}

    switchKeyMap = {'v': False}
    checkedSize = 0
    pressSet = set()
    actionQueue = None
    listener = None
    enable = False

    def __init__(self, actionQueue: Queue):
        Thread.__init__(self)
        self.actionQueue = actionQueue
        self.controller = None

    def run(self):
        with keyboard.Listener(win32_event_filter=self.filter, suppress=False) as self.listener:
            self.listener.join()

    def stop(self):
        self.actionQueue.put('_EXIT_')
        self.listener.stop()

    def filter(self, msg, data):
        if data.vkCode == self.VK_OEM_1 and msg == self.MSG_PRESS_WITH_ALT:
            if not self.enable:
                self.controller.getConfig()
            self.enable = not self.enable
        if data.vkCode == KeyCodeUtil.key2Vk(keyboard.Key.tab) and (
                msg == self.MSG_PRESS_WITH_ALT or msg == self.MSG_RELEASE_WITH_ALT):
            return
        if not self.enable:
            return
        if data.vkCode not in self.usedKeySet:
            return
        if msg == self.MSG_PRESS:
            self.on_press(KeyCodeUtil.vk2Char(data.vkCode))
            self.listener.suppress_event()
        elif msg == self.MSG_RELEASE:
            self.on_release(KeyCodeUtil.vk2Char(data.vkCode))
            self.listener.suppress_event()

    def on_press(self, key):
        if key in self.continuousSet or key in self.headTailTriggerSet:
            self.actionQueue.put('_P_' + key)
        elif key in self.pressTriggerSet:
            if key not in self.switchKeyMap:
                self.actionQueue.put('_P_' + key)
            else:
                self.switchKeyMap[key] = not self.switchKeyMap[key]
                switchKey = '_O_' + key if self.switchKeyMap[key] else '_C_' + key
                self.actionQueue.put(switchKey)

    def on_release(self, key):
        if key in self.continuousSet or key in self.headTailTriggerSet:
            self.actionQueue.put('_R_' + key)
            self.pressSet.discard(key)
