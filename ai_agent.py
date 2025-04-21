import argparse
import json
import requests
import urllib.parse

def process_input(user_input):
    # Define prompt template
    prompt = f"Convert this instruction to a single bash command: {user_input}"

    # URL-encode the prompt
    encoded_prompt = urllib.parse.quote(prompt)
    url = f"http://127.0.0.1:5500/?text={encoded_prompt}"

    try:
        # Send GET request to the GPT-4 Web API
        response = requests.get(url, timeout=10)
        response.raise_for_status()  # Raise exception for HTTP errors

        # Get response text
        response_text = response.text.strip()

        # Extract command from code block
        start_marker = "```bash\n"
        end_marker = "\n```"
        start_idx = response_text.find(start_marker)
        if start_idx == -1:
            raise ValueError("No bash code block found in API response")
        start_idx += len(start_marker)
        end_idx = response_text.find(end_marker, start_idx)
        if end_idx == -1:
            raise ValueError("Invalid bash code block format")

        command = response_text[start_idx:end_idx].strip()

        # Handle dynamic folder names (e.g., replace 'folder_name' with derived name)
        if "folder_name" in command:
            # Derive folder name from input (e.g., "create a project folder" -> "project_folder")
            derived_name = user_input.lower().replace("create a", "").strip().replace(" ", "_")
            if not derived_name:
                derived_name = "folder"  # Fallback
            command = command.replace("folder_name", derived_name)

        if not command:
            raise ValueError("Empty command returned by the API")

        # Return JSON response
        return {"input": user_input, "command": command}

    except requests.RequestException as e:
        return {"input": user_input, "error": f"API request failed: {str(e)}"}
    except ValueError as e:
        return {"input": user_input, "error": f"Invalid response: {str(e)}"}
    except Exception as e:
        return {"input": user_input, "error": f"Unexpected error: {str(e)}"}

def main():
    parser = argparse.ArgumentParser(description="AI Agent for converting instructions to bash commands")
    parser.add_argument("input", type=str, help="User instruction to convert to bash command")
    args = parser.parse_args()

    result = process_input(args.input)
    print(json.dumps(result))

if __name__ == "__main__":
    main()