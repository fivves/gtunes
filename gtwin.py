import json
import subprocess
import sys

pid = int(sys.argv[1])
out = subprocess.run(["hyprctl", "clients", "-j"], capture_output=True, text=True).stdout
for c in json.loads(out):
    if c.get("pid") == pid:
        print(c["address"])
        print(
            str(c["size"][0]) + "x" + str(c["size"][1]),
            "floating=" + str(int(c["floating"])),
            "title=" + str(c.get("title")),
        )
        break
