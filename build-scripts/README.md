# Set up continuous integration on the server

Create a CI user

```sh
ssh <username>@somerville.events
sudo adduser git
exit
```

Set up the required directory structure and bare git repo

```sh
mkdir artifacts srv worktrees bin repo.git
cd repo.git
git init --bare
exit
git remote add vps ssh://git@somerville.events:~/repo.git
git push vps main
```

Copy the build scripts to the server

```sh
git clone git@github.com:Somerville-Events/build-scripts.git
cd build-scripts
scp build.sh git@somerville.events:~/
scp post-receive git@somerville.events:~/repo.git/hooks/
scp somerville-events.service git@somerville.events:~/.config/systemd/
user
```

Create and fill out the .env file based on the instructions in https://github.com/Somerville-Events/somerville.events

```sh
nano .env
```

Give the scripts the correct permissions, reload the systemd config so it knows about the service script, and test the build script

```sh
ssh git@somerville.events
chmod a+x build.sh repo.git/hooks/post-receive
systemctl --user daemon-reload
./build.sh
```

Test the post-receive hook

```sh
# hook scripts take three inputs in this order
# <old commit hash> <new commit hash> <branch>
# <old commit hash> can be anything, like 000
echo "000 <most-recent-commit-hash> refs/heads/main" | repo.git/hooks/post-receive
# Watch the logs to observe it is building properly
journalctl --user -f -u build
```

After a successful build, test that the app service is running

```sh
systemctl --user status somerville-events
journalctl --user -f -u somerville-events
```

Ensure the server responds

```sh
curl localhost:8080
```
