# Introduction

This version of Buzz uses the [Dhall](https://dhall-lang.org) configuration language instead of TOML, and outputs JSON to standard output instead of interfacing with any system trays.

See [`buzz.dhall`](./buzz.dhall) for an example of the configuration.
You must place yours in your configuration folder, e.g. `~/.config/buzz.dhal` on Linux.

## Using with Waybar
I use Buzz with the following configuration:

```jsonc
"custom/buzz": {
  "format": "{}",
  "exec": "buzz",
  "return-type": "json"
}
```

`"return-type": "json"` expects a certain format which I have implemented in this branch. Buzz's output to stdout is demonstrated below:

```json
{"text":"","tooltip":"You have reached inbox 0!","class":"mail-read","percentage":0.0}
```

See [waybar custom module wiki](https://github.com/alexays/waybar/wiki/Module:-Custom) for more information about how to use this.