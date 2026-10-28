# Assets

`face_detection_yunet_2023mar.onnx` is YuNet, the face detector from
[OpenCV Zoo](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet).
It is MIT licensed, copyright Shiqi Yu and the OpenCV Zoo contributors.
It is built into the exe with `include_bytes!`, so the app needs no
download at setup and never uses the network.
