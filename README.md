# Universal authentication proxy

Your API keys are yours: don't give them to an agent.

## Try this

Get `lr` here: [localhost-router](https://github.com/nillebco/localhost-router).
Get `fnox` here: [fnox](https://github.com/jdx/fnox)
Prepare a fnox.toml (you might require `rbw`, `bitwarden-cli`, or any supported secrets manager).
Copy the config.example.toml to a config.toml

```sh
# only once, to set up an host name associated with this service
# you can forward as many hosts as you want to the same port
lr add llm 8123

fnox exec cargo run

# in a different terminal
curl -skv https://llm.localhost/v1/models
```
