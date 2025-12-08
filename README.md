# immichctl

immichctl is a command line tool to manage [Immich](https://docs.immich.app) assets and implement missing UI functions.

## General

`immichctl <operation/command/verb> <type> <options>`

- command/verb: get, create, delete, add, remove, adjust, login, version ...
- type: selection, tag, album

## Server Commands

### Login

`immichctl login <SERVER> --apikey <apikey>`

- connect to the Immich server
- login information is stored in `$HOME/.immichctl/config.json`

### Version

`immichctl version`

- prints out `immichctl` version and, if connected, the server version

### Logout

`immichctl logout`

- remove login information

## Manage Asset Selections

Most immichctl commands like adding/removing tags, adjusting timestamps etc. work on asset selections.
The current asset selection is stored in `$HOME/.immchctl/selection.json`. 

### List selection

`immichctl list selection`

### Clear selection

`immichctl clear selection`

### Add assets to selection

Single asset by id:
`immichctl add selection --id <asset id>`
`immichctl add selection --tag <tag>`

### Count selection

`immichctl count selection`
