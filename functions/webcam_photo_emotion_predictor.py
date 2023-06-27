import cv2
import os
from functions.get_emotion import get_emotion


def webcam_photo_emotion_predictor():
    # Open the webcam
    cap = cv2.VideoCapture(0)

    # Capture photo
    ret, frame = cap.read()

    # Release the webcam
    cap.release()

    # Save the photo
    photo_path = "temp/webcam_photo.jpg"
    cv2.imwrite(photo_path, frame)

    try:
        # Get emotion from the photo
        emotion = get_emotion(photo_path)
    except Exception as e:
        print(e)
        return 'neutral'

    # Delete the photo
    os.remove(photo_path)

    return emotion
