# Universal authentication proxy

Your API keys are yours: don't give them to an agent.

## Try this

Get `lr` here: [localhost-router](https://github.com/nillebco/localhost-router).

Get `fnox` here: [fnox](https://github.com/jdx/fnox)

Copy `fnox.example.toml` to `fnox.toml` (you might require `rbw`, `bitwarden-cli`, or any supported secrets manager), update it to match your preferences.

Copy the `config.example.toml` to a `config.toml`, update it to match your hostnames.

```sh
# only once, to set up an host name associated with this service
# you can forward as many hosts as you want to the same port
lr add openai 8123
lr add claude 8123

fnox exec cargo run

# quick test: in a different terminal (this is what your agents will do)
curl -skv https://openai.localhost/v1/models

# now launch your favourite tool
OPENAI_BASE_URL=https://openai.localhost ANTHROPIC_BASE_URL=https://claude.localhost claude
```
