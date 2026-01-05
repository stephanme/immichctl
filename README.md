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

### Curl

`immichctl curl /server/version`
`immichctl curl --method post /search/metadata -data '{"id":"<uuid>"}'`

- see [Immich API doc](https://api.immich.app/introduction)
- takes care for authentication and immich API url prefix
- prints out json response on success
- use `RUST_LOG=trace` for debugging (very verbose)

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

`immichctl assets list --help` - for all options

### Clear asset selection

`immichctl assets clear`

### Count assets selection

`immichctl assets count`

### Refresh assets selection

Refreshes the metadata of the assets selection. Loads additional metadata like exif metadata.
Requires one request per assets, i.e. the operation can be slow.

`immichctl assets refresh`

### Adjust assets date, time and timezone info

Allows to correct a misconfigured timezone or date/time settings of a camera.
The current/base timestamp and timezone is taken from exif `dateTimeOriginal` and `timeZone` if available otherwise from asset metadata (`fileCreatedAt` and `localDateTime`).
Check the change with `--dry-run` before applying.

`immichctl assets datatime --dry-run` - prints timestamp instead of changing it

`immichctl assets datatime --timezone <timezone offset>` - set timezone (e.g. to +02:00)
`immichctl assets datatime --offset <offset>` - adjust timestamp by an offset (e.g. -1d2h30m)

## Tag Commands

Tags can be assigned/unassigned to the selected assets. Tags are not created or deleted, i.e. the tag must already exist before it can be added.
Tags can be specified with full hierarchical name (e.g. `parent/child`) or with just the tag name (`child`) if the name is unambiguous.

### Assign tag to assets

`immichctl tag assign <tag name>`

### Unassing tag from assets

`immichctl tag unassign <tag name>`
