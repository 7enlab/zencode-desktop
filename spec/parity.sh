B=$1; d=$(dirname "$0")/cases; pass=0; fail=0
for j in "$d"/*.json; do n=${j%.json}; mapfile -d '' -t A < <(python3 -c 'import json,sys;[print(a,end="\0") for a in json.load(open(sys.argv[1]))]' "$j")
 if [ -f "$n.stdin" ]; then got=$("$B" "${A[@]}" < "$n.stdin"; echo x); else got=$("$B" "${A[@]}" </dev/null; echo x); fi
 exp=$(cat "$n.out"; echo x); if [ "$got" == "$exp" ]; then pass=$((pass+1)); else fail=$((fail+1)); echo "FAIL $(basename $n)"; diff <(echo "$exp") <(echo "$got") | head -8; fi; done
echo "pass=$pass fail=$fail"; [ $fail -eq 0 ]
