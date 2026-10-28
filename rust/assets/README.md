# Assets

`face_detection_yunet_2023mar.onnx` is YuNet, the face detector from
[OpenCV Zoo](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet).
It is MIT licensed, copyright Shiqi Yu and the OpenCV Zoo contributors.
It is built into the exe with `include_bytes!`, so the app needs no
download at setup and never uses the network.

`Tiny5-Regular.ttf` is the whisper and panel font, from
[Gissio/font_Tiny5](https://github.com/Gissio/font_Tiny5). It is licensed
under the SIL Open Font License 1.1 (`Tiny5-OFL.txt`), copyright The Tiny5
Project Authors.

`blinki.json` holds the moods, exported from the Blinki Studio.
