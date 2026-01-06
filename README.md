# immichctl

immichctl is a command line tool to manage [Immich](https://docs.immich.app) assets and implement missing UI functions.

immichctl doesn't handle upload and download of assets as there are command line tools like [immich-go](https://github.com/simulot/immich-go) that implement this perfectly.

Main use cases:
- fix timestamps and time zone of assets (e.g. because camera time was not correct)
- check/fix missing tags after image upload caused by [immich-go #990](https://github.com/simulot/immich-go/issues/990) / [immich #16747](https://github.com/immich-app/immich/issues/16747)
- rename/re-assign tags

## General

`immichctl <subject> <operation/command/verb> <options>`

- subject: assets, tag, album, ...
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

```
immichctl curl /server/version
immichctl curl --method post /search/metadata -data '{"id":"<uuid>"}'
```

- see [Immich API doc](https://api.immich.app/introduction)
- takes care for authentication and immich API url prefix
- prints out json response on success
- use `RUST_LOG=trace` for debugging (very verbose)

## Manage Assets

Most immichctl commands like assigning tags, adjusting timestamps etc. work on an asset selection.
The current asset selection is stored in `$HOME/.immchctl/assets.json`. 

### Search for assets

The assets returned by the Immich search are added to the asset selection.

Single asset by id:<br/>
`immichctl assets search --id <asset id>`

Tagged assets:<br/>
`immichctl assets search --tag <tag>`

Assets of an album:<br/>
`immichctl assets search --album <album>`

### Remove assets from selection

When `--remove` is specified, the assets returned by the Immich search are removed from the asset selection. E.g.:

`immichctl assets serach --remove --tag <tag>`

### List assets

```
immichctl assets list
immichctl assets list -c id -c file -c datetime
immichctl assets list --format csv -c created -c timezone

immichctl assets list --format json
immichctl assets list --format json-pretty

# for all options
immichctl assets list --help
```

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

Prints timestamp instead of changing it:<br/>
`immichctl assets datatime --dry-run`

Set timezone (e.g. to +02:00):<br/>
`immichctl assets datatime --timezone <timezone offset>`

Adjust timestamp by an offset (e.g. -1d2h30m):<br/>
`immichctl assets datatime --offset <offset>`

## Tag Commands

Tags can be assigned/unassigned to the selected assets. Tags are not created or deleted, i.e. the tag must already exist before it can be added.
Tags can be specified with full hierarchical name (e.g. `parent/child`) or with just the tag name (`child`) if the name is unambiguous.

### Assign tag to assets

`immichctl tag assign <tag name>`

### Unassing tag from assets

`immichctl tag unassign <tag name>`
