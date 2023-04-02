#!/bin/bash
set -eu -o pipefail

mkdir basics;

function baseline() {
  {
    echo "$1"
    git -c core.attributesFile=user.attributes check-attr -a "$1"
    echo
  } >> baseline
}


(cd basics
  git init

  # based on https://github.com/git/git/blob/140b9478dad5d19543c1cb4fd293ccec228f1240/t/t0003-attributes.sh#L45
	mkdir -p a/b/d a/c b
	(
		echo "[attr]notest !test"
		echo "\" d \"	test=d"
		echo " e	test=e"
		echo " e\"	test=e"
		echo "f	test=f"
		echo "a/i test=a/i"
		echo "onoff test -test"
		echo "offon -test test"
		echo "no notest"
		echo "A/e/F test=A/e/F"
	) > .gitattributes
	(
		echo "g test=a/g"
		echo "b/g test=a/b/g"
	) > a/.gitattributes
	(
		echo "h test=a/b/h"
		echo "d/* test=a/b/d/*"
		echo "d/yes notest"
	) > a/b/.gitattributes
	(
		echo "global test=global"
	) > user.attributes

	git add . && git commit -qm c1

  baseline " d "
  baseline e
  baseline "e\""
  baseline a/i
  baseline onoff
  baseline offon
  baseline no
  baseline A/e/F
  baseline a/g
  baseline a/b/g
  baseline a/b/h
  baseline a/b/d/ANY
  baseline a/b/d/yes
  baseline global
)
