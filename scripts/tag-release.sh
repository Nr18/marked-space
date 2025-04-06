set -xe
VERSION=$(grep -m 1 '^version' Cargo.toml | sed 's/^version = "\(.*\)"$/\1/')
MAJOR=$(echo $VERSION | cut -d. -f1)
MINOR=$(echo $VERSION | cut -d. -f2)
PATCH=$(echo $VERSION | cut -d. -f3)

FORCE=-f

echo $VERSION
git tag $FORCE v$VERSION
git tag $FORCE v$MAJOR.$MINOR
git tag $FORCE v$MAJOR

git push origin v$VERSION v$MAJOR.$MINOR v$MAJOR $FORCE
