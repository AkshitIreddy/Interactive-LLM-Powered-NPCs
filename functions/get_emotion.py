from deepface import DeepFace


def get_emotion(image_path):
    objs = DeepFace.analyze(img_path=image_path, actions=[
                            'emotion'], silent='True', detector_backend='retinaface')
    emotion = objs[0]['dominant_emotion']
    return emotion
