import speech_recognition as sr

def speech_to_text(audio):
    r = sr.Recognizer()
    text = "NULL"
    error = "NULL"
    try:
        text = r.recognize_google(audio)
    except sr.UnknownValueError:
        error = "Audio is silent or Google Speech Recognition could not understand audio"
    except sr.RequestError as e:
        error = f"Could not request results from Google Speech Recognition service; {e}"
    
    return text, error