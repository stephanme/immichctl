# immichctl

immichctl is a command line tool to manage [Immich](https://docs.immich.app) assets and implement missing UI functions.

immichctl doesn't handle upload and download of assets as there are command line tools like [immich-go](https://github.com/simulot/immich-go) that implement this perfectly.

## General

`immichctl <subject> <operation/command/verb> <options>`

- type: selection, tag, album, timestamp
- command/verb: list, create, delete, add, remove, adjust, login, version ...

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

`immichctl selection list`
`immichctl selection list -c id -c file -c datetime`
`immichctl selection list --format csv -c created -c timezone`

`immichctl selection list --format json`
`immichctl selection list --format json-pretty`


### Clear selection

`immichctl selection clear`

### Count selection

`immichctl selection count`

### Add assets to selection

Single asset by id:
`immichctl selection add --id <asset id>`

Tagged assets:
`immichctl selection add --tag <tag>`

Assets of an album:
`immichctl selection add --album <album>`

### Remove assets from selection

Single asset by id:
`immichctl selection remove --id <asset id>`

Tagged assets:
`immichctl selection remove --tag <tag>`

Assets of an album:
`immichctl selection remove --album <album>`

## Tag Commands

Tags can be assigned/unassigned to the selected assets. Tags are not created or deleted, i.e. the tag must already exist before it can be added.
Tags can be specified with full hierarchical name (e.g. `parent/child`) or with just the tag name (`child`) if the name is unambiguous.

### Add tag to selection

`immichctl tag add <tag>`

### Remove tag from selection

`immichctl tag remove <tag>`