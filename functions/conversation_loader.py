import json
from functions.token_length import token_len
import random
from langchain.vectorstores import Chroma
from langchain.embeddings import CohereEmbeddings
from langchain.llms import Cohere
from langchain import PromptTemplate, LLMChain


def conversation_loader(transcribed_text, player_name, game_name, character_name):
    # Load the existing conversation from conversation.json
    with open(f'{game_name}/characters/{character_name}/conversation.json', 'r') as f:
        conversation = json.load(f)['conversation']

    conversation_string = ''
    for line in conversation:
        conversation_string += line['sender'] + ": " + line['message'] + '\n'

    token_length = token_len(conversation_string)

    if token_length > 500 and character_name == 'default':
        # Clear the conversation and add the new line
        conversation = [{'sender': player_name, 'message': transcribed_text}]
        conversation_string = f"{player_name}: {transcribed_text}\n"

    elif token_length > 500:
        # Randomly select an API key
        selected_key = json.load(open('apikeys.json', 'r'))['api_keys'][random.randint(
            0, len(json.load(open('apikeys.json', 'r'))['api_keys'])-1)]

        # Initialise model
        llm = Cohere(cohere_api_key=selected_key,
                     model='command', temperature=0.9, max_tokens=300)

        # create the template string
        template = """{conversation_string}\n\nSummarize the above conversation in detail. The summary must be very descriptive."""

        # create prompt
        prompt = PromptTemplate(template=template, input_variables=[
                                'conversation_string'])

        # Create and run the llm chain
        llm_chain = LLMChain(prompt=prompt, llm=llm)
        summary = llm_chain.run(conversation_string=conversation_string)

        # Initialise embeddings model
        embeddings = CohereEmbeddings(cohere_api_key=selected_key)

        # Set the path to the character's persist_directory
        persist_directory = f'{game_name}/characters/{character_name}/vectordb'

        # Initialise the vectordb
        vectordb = Chroma(persist_directory=persist_directory,
                          embedding_function=embeddings)

        # Add the summary
        vectordb.add_texts([summary])
        vectordb.persist()

        # Clear the conversation and add the new line
        conversation = [{'sender': player_name, 'message': transcribed_text}]
        conversation_string = f"{player_name}: {transcribed_text}\n"
    else:
        # Append the new line to the conversation
        conversation.append(
            {'sender': player_name, 'message': transcribed_text})
        conversation_string += f"{player_name}: {transcribed_text}\n"

    # Save the updated conversation back to conversation.json
    with open(f'{game_name}/characters/{character_name}/conversation.json', 'w') as f:
        json.dump({'conversation': conversation}, f)

    return conversation_string
