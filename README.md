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

## Manage Assets

Most immichctl commands like assigning tags, adjusting timestamps etc. work on an asset selection.
The current asset selection is stored in `$HOME/.immchctl/assets.json`. 

### Search for assets

The assets returned by the Immich search are added to the asset selection.

Single asset by id:
`immichctl assets search --id <asset id>`

Tagged assets:
`immichctl assets search --tag <tag>`

Assets of an album:
`immichctl assets search --album <album>`

### Remove assets from selection

When `--remove` is specified, the assets returned by the Immich search are removed from the asset selection. E.g.:

`immichctl assets serach --remove --tag <tag>`

### List assets

`immichctl assets list`
`immichctl assets list -c id -c file -c datetime`
`immichctl assets list --format csv -c created -c timezone`

`immichctl assets list --format json`
`immichctl assets list --format json-pretty`

### Clear asset selection

`immichctl assets clear`

### Count assets selection

`immichctl assets count`


## Tag Commands

Tags can be assigned/unassigned to the selected assets. Tags are not created or deleted, i.e. the tag must already exist before it can be added.
Tags can be specified with full hierarchical name (e.g. `parent/child`) or with just the tag name (`child`) if the name is unambiguous.

### Assign tag to assets

`immichctl tag assign <tag name>`

### Unassing tag from assets

`immichctl tag unassign <tag name>`