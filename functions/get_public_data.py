from langchain.embeddings import CohereEmbeddings
import json
import random
from langchain.vectorstores import Chroma


def get_public_data(game_name, character_name):

    selected_key = json.load(open('apikeys.json', 'r'))['api_keys'][random.randint(
        0, len(json.load(open('apikeys.json', 'r'))['api_keys'])-1)]

    embeddings = CohereEmbeddings(cohere_api_key=selected_key)

    persist_directory = f'{game_name}/public_vectordb'

    vectordb = Chroma(persist_directory=persist_directory,
                      embedding_function=embeddings)

    # Load the existing conversation from conversation.json
    with open(f'{game_name}/characters/{character_name}/conversation.json', 'r') as f:
        conversation = json.load(f)['conversation']

    if len(conversation) > 5:
        conversation = conversation[-5:]

    conversation_string = ''
    for line in conversation:
        conversation_string += line['sender'] + ": " + line['message'] + '\n'

    docs = vectordb.similarity_search(conversation_string, k=1)
    public_info_string = "\n".join(doc.page_content for doc in docs)

    return public_info_string
