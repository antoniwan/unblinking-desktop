# Assets

`face_detection_yunet_2023mar.onnx` is YuNet, the face detector from
[OpenCV Zoo](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet).
It is MIT licensed, copyright Shiqi Yu and the OpenCV Zoo contributors.
It is built into the exe with `include_bytes!`, so the app needs no
download at setup and never uses the network.

`facial_expression_recognition_mobilefacenet_2022july.onnx` is the facial
expression recogniser from
[OpenCV Zoo](https://github.com/opencv/opencv_zoo/tree/main/models/facial_expression_recognition)
(Progressive Teacher, by Jing Jiang, on a MobileFaceNet; ONNX by Chengrui
Wang). It is licensed under the Apache License 2.0 (`Apache-2.0.txt`),
copyright the Shenzhen Institute of Artificial Intelligence and Robotics for
Society and the OpenCV Zoo contributors. It is unmodified (sha256
`4f61307602fc089ce20488a31d4e4614e3c9753a7d6c41578c854858b183e1a9`), 4.8 MB,
and built into the exe like YuNet. It only runs while the user has turned on
expression reading, on faces cut from frames that are dropped right after.
The smaller int8 version in the zoo uses block quantisation, which tract
can't load yet.

`Tiny5-Regular.ttf` is the whisper and panel font, from
[Gissio/font_Tiny5](https://github.com/Gissio/font_Tiny5). It is licensed
under the SIL Open Font License 1.1 (`Tiny5-OFL.txt`), copyright The Tiny5
Project Authors.

`blinki.json` holds the moods, exported from the Blinki Studio.

`icon.png` is the app icon for the installer, the one the Electron app's
`npm run setup` draws.
