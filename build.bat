E:\vimouse\venv\Scripts\pyinstaller.exe --clean --noconfirm --onedir --windowed --name "vimouse" --icon "E:/vimouse/logo.ico" --add-data "E:/vimouse/src/core;core/" --add-data "E:/vimouse/src/controller;controller/"  --add-data "E:/vimouse/src/utils;utils/" --paths "E:/vimmouse/Lib/site-packages" --distpath "E:/vimouse/dist/" --hidden-import=json --debug=imports "E:/vimouse/src/main.py"