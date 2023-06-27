def get_name(transcribed_text, characters):
    characters = [word.replace('_', ' ') for word in characters]
    for name in characters:
        # Check for full name match
        if name == transcribed_text:
            return name
        name_parts = name.split()
        for part in name_parts:
            if transcribed_text.startswith(part):
                return name
    return None
