# -*- coding: UTF-8 -*-
import os
import json
from core.keyboardinterceptor import KeyboardInterceptor
from queue import Queue
from core.keyhandler import KeyHandler

DEFAULT_CONFIG = {
    "buffCoef1": 0.3,
    "buffCoef2": 0.6,
    "buffCoef3": 4.0,
    "buffCoef4": 8.0,
    "buffCoef5": 12.0,
    "shiftBuff": 2,
    "spaceBuff": 3
}


class AppController:
    queue = None
    interceptor = None
    keyHandler = None
    engaged = False
    # ui = None

    def __init__(self):
        pass

    def start(self):
        self.queue = Queue()
        self.interceptor = KeyboardInterceptor(self.queue)
        self.interceptor.controller = self
        self.keyHandler = KeyHandler(self.interceptor, self.queue, self.getConfig())
        self.interceptor.start()
        self.keyHandler.start()
        self.engaged = True

    def stop(self):
        self.keyHandler.stop()
        self.interceptor.stop()
        self.engaged = False

    def startFilter(self):
        self.interceptor.enable = True

    def stopFilter(self):
        self.interceptor.enable = False

    @staticmethod
    def getConfig():
        if not os.path.exists('./config.json'):
            return DEFAULT_CONFIG
        with open('config.json', 'r', encoding='utf8') as f:
            configData = json.load(f)
        return configData


if __name__ == '__main__':
    print("controller Module")
