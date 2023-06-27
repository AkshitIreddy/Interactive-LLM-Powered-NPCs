from deepface import DeepFace


def get_age_gender_race(image_path):
    objs = DeepFace.analyze(img_path=image_path, actions=[
                            'age', 'gender', 'race'], silent=True, detector_backend='retinaface')
    age = objs[0]['age']
    gender = objs[0]['dominant_gender']
    race = objs[0]['dominant_race']
    return age, gender, race
