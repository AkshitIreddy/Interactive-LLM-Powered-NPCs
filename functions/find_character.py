from deepface import DeepFace


def find_character_with_lowest_cosine_score(game_name, characters_list, image_path):
    lowest_score = float('inf')
    character_with_lowest_score = 'NULL'

    for character_name in characters_list:
        db_path = f"{game_name}/characters/{character_name}/images"
        print(db_path)
        dfs = DeepFace.find(img_path=image_path,
                            db_path=db_path, model_name='Facenet512', detector_backend='retinaface')

        if len(dfs[0]) != 0:
            cosine_score = dfs[0]['Facenet512_cosine'][0]
            if cosine_score < lowest_score:
                lowest_score = cosine_score
                character_with_lowest_score = character_name

    return character_with_lowest_score
