import cv2
from deepface import DeepFace


def save_extracted_face(image_path, output_path="face.jpg"):
    # Load the image using OpenCV
    image = cv2.imread(image_path)

    try:
        # Extract faces using DeepFace
        face_objs = DeepFace.extract_faces(
            img_path=image_path,
            detector_backend='retinaface',
        )
    except Exception as e:
        print("No Faces Detected")
        return 'NULL', 'NULL'

    # Check the number of detected faces
    num_faces = len(face_objs)
    print(face_objs)
    if num_faces == 1:
        print("One Face Detected")
        # Extract the facial area coordinates
        face = face_objs[0]
        x, y, w, h = (
            face['facial_area']['x'],
            face['facial_area']['y'],
            face['facial_area']['w'],
            face['facial_area']['h']
        )

        # Calculate the center of the face region
        center_x = x + (w // 2)
        center_y = y + (h // 2)

        # Calculate the extended boundaries of the face region
        extended_x = max(0, center_x - int(w * 1.5))
        extended_y = max(0, center_y - int(h * 1.5))
        extended_w = min(int(w * 3), image.shape[1] - extended_x)
        extended_h = min(int(h * 3), image.shape[0] - extended_y)

        # Crop the extended face region from the image
        extracted_face = image[extended_y:extended_y +
                               extended_h, extended_x:extended_x + extended_w]

        # Save the extracted face to the output path
        cv2.imwrite(output_path, extracted_face)

        return output_path, (extended_x, extended_y, extended_w, extended_h)
    else:
        print("Multiple Faces Detected")
        # Find the face with the highest face_confidence
        highest_confidence_face = max(
            face_objs, key=lambda face: face['confidence'])

        # Extract the facial area coordinates
        x, y, w, h = (
            highest_confidence_face['facial_area']['x'],
            highest_confidence_face['facial_area']['y'],
            highest_confidence_face['facial_area']['w'],
            highest_confidence_face['facial_area']['h']
        )

        # Calculate the center of the face region
        center_x = x + (w // 2)
        center_y = y + (h // 2)

        # Calculate the extended boundaries of the face region
        extended_x = max(0, center_x - int(w * 1.5))
        extended_y = max(0, center_y - int(h * 1.5))
        extended_w = min(int(w * 3), image.shape[1] - extended_x)
        extended_h = min(int(h * 3), image.shape[0] - extended_y)

        # Crop the extended face region from the image
        extracted_face = image[extended_y:extended_y +
                               extended_h, extended_x:extended_x + extended_w]

        # Save the extracted face to the output path
        cv2.imwrite(output_path, extracted_face)

        return output_path, (extended_x, extended_y, extended_w, extended_h)
