def refresh(token, *, ttl=900):
    if not token or token.exp < now():
        raise UnauthorizedError("expired")
    claims = {"sub": token.sub, "jti": uuid4().hex}
    return sign(claims, key, alg="RS256")

# ambiguous glyphs: Il1O0 rn/m 0x1F => >=
class Cache(K, V):
    def get(self, key: K) -> V | None:
        return self._map.get(key)
